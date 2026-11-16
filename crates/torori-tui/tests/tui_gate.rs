//! torori-tui gate tests. Roughly half validation, half
//! adversarial (per the estate 50/50 split): plan-mode no-call
//! proof, oversized bundles, malformed rows, dying backend,
//! lane-label fail-closed cases, idempotency, depth caps.

use std::cell::RefCell;
use std::rc::Rc;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serde_json::{Value, json};
use torori_tui::app::{App, Mode, RunEvent, Screen, Workflow};
use torori_tui::facade::{FacadeResult, FakeFacade, TororiFacade, parse_rows};
use torori_tui::panels;
use torori_tui::triggers::{AuditOnlyDispatcher, TriggerEngine, TriggerEvent, candidate_args};

/// Shared fake so tests can inspect recorded calls after the
/// facade moves into the App.
#[derive(Clone)]
struct SharedFake(Rc<RefCell<FakeFacade>>);

impl TororiFacade for SharedFake {
    fn status(&mut self) -> FacadeResult<Value> {
        self.0.borrow_mut().status()
    }
    fn tools(&mut self) -> FacadeResult<Vec<String>> {
        self.0.borrow_mut().tools()
    }
    fn catalogue_search(&mut self, q: &str, l: u64) -> FacadeResult<Value> {
        self.0.borrow_mut().catalogue_search(q, l)
    }
    fn catalogue_stats(&mut self) -> FacadeResult<Value> {
        self.0.borrow_mut().catalogue_stats()
    }
    fn pair_validate(&mut self, p: Value) -> FacadeResult<Value> {
        self.0.borrow_mut().pair_validate(p)
    }
    fn bundle_validate(&mut self, d: &str) -> FacadeResult<Value> {
        self.0.borrow_mut().bundle_validate(d)
    }
    fn bundle_ingest(&mut self, d: &str) -> FacadeResult<Value> {
        self.0.borrow_mut().bundle_ingest(d)
    }
    fn bundle_list(&mut self) -> FacadeResult<Value> {
        self.0.borrow_mut().bundle_list()
    }
    fn voice_list(&mut self) -> FacadeResult<Value> {
        self.0.borrow_mut().voice_list()
    }
    fn voice_lane_state(&mut self) -> FacadeResult<String> {
        self.0.borrow_mut().voice_lane_state()
    }
    fn voice_generate(&mut self, a: Value) -> FacadeResult<Value> {
        self.0.borrow_mut().voice_generate(a)
    }
}

fn engine() -> TriggerEngine {
    TriggerEngine::embedded_default().expect("embedded rules parse")
}

fn app_with(fake: &SharedFake) -> App {
    App::new(Box::new(fake.clone()), engine())
}

fn search_payload() -> Value {
    json!({"total": 2, "items": [
        {"slug": "act-a", "title": "A", "kind": "ACT", "status": "PLANNED"},
        {"slug": "kink-b", "title": "B", "kind": "KINK", "status": "GAP"}
    ]})
}

// ── Validation ────────────────────────────────────────────────

#[test]
fn plan_mode_stages_without_any_facade_call() {
    let fake = SharedFake(Rc::new(RefCell::new(FakeFacade::new())));
    let mut app = app_with(&fake);
    let events = app.run(Workflow::CatalogueSearch {
        query: "breath".to_string(),
    });
    assert!(matches!(events[0], RunEvent::Log(_)));
    assert!(app.staged.is_some());
    assert!(
        fake.0.borrow().calls.is_empty(),
        "plan mode must make no call before approval"
    );
    let staged = app.staged.as_ref().expect("staged");
    assert_eq!(staged.tool, "torori_catalogue_search");
    assert_eq!(staged.args, json!({"query": "breath"}));
}

#[test]
fn approve_executes_exactly_the_staged_call() {
    let fake = SharedFake(Rc::new(RefCell::new(FakeFacade::new())));
    fake.0
        .borrow_mut()
        .script("catalogue_search", vec![search_payload()]);
    let mut app = app_with(&fake);
    let _ = app.run(Workflow::CatalogueSearch {
        query: "breath".to_string(),
    });
    let events = app.approve_staged();
    assert!(matches!(events.last(), Some(RunEvent::Done(_))));
    let calls = &fake.0.borrow().calls;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "catalogue_search");
    assert_eq!(app.last_rows.len(), 2);
    assert_eq!(app.last_rows[0].slug, "act-a");
}

#[test]
fn auto_mode_search_populates_rows() {
    let fake = SharedFake(Rc::new(RefCell::new(FakeFacade::new())));
    fake.0
        .borrow_mut()
        .script("catalogue_search", vec![search_payload()]);
    let mut app = app_with(&fake);
    app.mode = Mode::Auto;
    let _ = app.run(Workflow::CatalogueSearch {
        query: String::new(),
    });
    assert_eq!(app.last_rows.len(), 2);
    assert_eq!(app.last_skipped, 0);
}

#[test]
fn ingest_emits_bounded_progress_steps() {
    let dir = std::env::temp_dir().join("torori-tui-test-bundle-ok");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("bundle.ron"), "()").expect("write");
    let fake = SharedFake(Rc::new(RefCell::new(FakeFacade::new())));
    fake.0
        .borrow_mut()
        .script("bundle_validate", vec![json!({"valid": true})]);
    fake.0
        .borrow_mut()
        .script("bundle_ingest", vec![json!({"bundle_id": "b1"})]);
    let mut app = app_with(&fake);
    app.mode = Mode::Auto;
    let events = app.run(Workflow::BundleIngest {
        dir: dir.display().to_string(),
    });
    let progress: Vec<&RunEvent> = events
        .iter()
        .filter(|e| matches!(e, RunEvent::Progress { .. }))
        .collect();
    assert_eq!(progress.len(), 3);
    assert!(matches!(events.last(), Some(RunEvent::Done(_))));
    let methods: Vec<String> = fake
        .0
        .borrow()
        .calls
        .iter()
        .map(|(m, _)| m.clone())
        .collect();
    assert_eq!(methods, vec!["bundle_validate", "bundle_ingest"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn voice_job_stages_with_exact_args() {
    let fake = SharedFake(Rc::new(RefCell::new(FakeFacade::new())));
    let mut app = app_with(&fake);
    let staged = app
        .stage(Workflow::VoiceJob {
            conditioning_json: "{\"voice_id\":\"v1\"}".to_string(),
            slot_class: "whisper".to_string(),
            takes: 2,
        })
        .expect("stage");
    assert_eq!(staged.tool, "torori_voice_generate");
    assert_eq!(staged.args["slot_class"], json!("whisper"));
    assert_eq!(staged.args["takes"], json!(2));
    assert!(fake.0.borrow().calls.is_empty());
}

#[test]
fn screens_render_on_test_backend() {
    let fake = SharedFake(Rc::new(RefCell::new(FakeFacade::new())));
    fake.0.borrow_mut().script(
        "status",
        vec![json!({
            "name": "torori", "version": "1.0.1",
            "counts": {"catalogue_entries": 114, "library_rows": 3,
                       "bundles_ingested": 1, "pairs": 9,
                       "voice_identities": 2},
            "lanes": {"qwen_tts": "staged", "dsp_sfx": "available",
                      "route_c_comp": "available"},
            "silence_rule": "enforced"
        })],
    );
    fake.0
        .borrow_mut()
        .script("voice_lane_state", vec![json!({"lane": "staged"})]);
    fake.0.borrow_mut().script(
        "tools",
        vec![json!({"tools": ["torori_status", "torori_catalogue_search"]})],
    );
    let mut app = app_with(&fake);
    app.refresh_overview();
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("terminal");
    for screen in [
        Screen::Overview,
        Screen::Workflows,
        Screen::Log,
        Screen::Fleet,
    ] {
        app.screen = screen;
        terminal
            .draw(|frame| torori_tui::screens::render(frame, &app))
            .expect("draw");
        let text = buffer_text(terminal.backend().buffer().clone());
        match screen {
            Screen::Overview => {
                assert!(text.contains("torori"), "overview: {text}");
                assert!(text.contains("114"), "overview counts: {text}");
            }
            Screen::Fleet => {
                assert!(text.contains("ntranch"), "fleet stamps: {text}");
                assert!(text.contains("nsfw"), "fleet labels: {text}");
            }
            _ => {}
        }
    }
}

fn buffer_text(buffer: ratatui::buffer::Buffer) -> String {
    let mut out = String::new();
    for cell in buffer.content() {
        out.push_str(cell.symbol());
    }
    out
}

#[test]
fn line_mode_status_and_unknown_command() {
    let fake = SharedFake(Rc::new(RefCell::new(FakeFacade::new())));
    fake.0
        .borrow_mut()
        .script("status", vec![json!({"name": "torori"})]);
    let mut app = app_with(&fake);
    let out = app.handle_line("status");
    assert!(out[0].contains("torori"));
    let out = app.handle_line("frobnicate");
    assert!(out[0].contains("unknown command"));
    let out = app.handle_line("triggers");
    assert!(out.iter().any(|line| line.contains("nsfw")));
}

#[test]
fn embedded_rules_carry_birth_stamps() {
    let eng = engine();
    assert_eq!(eng.rules().len(), 2);
    for rule in eng.rules() {
        assert_eq!(rule.origin_lane, "ntranch");
        assert!(!rule.content_label_reason.is_empty());
        assert!(rule.max_depth <= 2);
    }
}

// ── Adversarial ───────────────────────────────────────────────

#[test]
fn approval_then_abort_makes_no_call() {
    let fake = SharedFake(Rc::new(RefCell::new(FakeFacade::new())));
    let mut app = app_with(&fake);
    let _ = app.run(Workflow::BundleIngest {
        dir: "/nonexistent".to_string(),
    });
    app.abort_staged();
    let events = app.approve_staged();
    assert!(matches!(events[0], RunEvent::Log(_)));
    assert!(fake.0.borrow().calls.is_empty());
}

#[test]
fn malformed_catalogue_rows_are_skipped_not_rendered() {
    let payload = json!({"items": [
        {"slug": "ok", "title": "OK", "kind": "ACT", "status": "GAP"},
        {"slug": "no-title", "kind": "ACT", "status": "GAP"},
        {"title": "no slug at all"},
        "a bare string",
        42
    ]});
    let (rows, skipped) = parse_rows(&payload).expect("parse");
    assert_eq!(rows.len(), 1);
    assert_eq!(skipped, 4);
}

#[test]
fn malformed_search_payload_fails_typed_not_panics() {
    let fake = SharedFake(Rc::new(RefCell::new(FakeFacade::new())));
    fake.0
        .borrow_mut()
        .script("catalogue_search", vec![json!({"unexpected": true})]);
    let mut app = app_with(&fake);
    app.mode = Mode::Auto;
    let events = app.run(Workflow::CatalogueSearch {
        query: "x".to_string(),
    });
    assert!(matches!(events.last(), Some(RunEvent::Failed(_))));
}

#[test]
fn backend_dying_mid_call_reports_failed_and_app_survives() {
    let fake = SharedFake(Rc::new(RefCell::new(FakeFacade::new())));
    fake.0.borrow_mut().fail_with = Some("server closed".to_string());
    let mut app = app_with(&fake);
    app.mode = Mode::Auto;
    let events = app.run(Workflow::CatalogueSearch {
        query: "x".to_string(),
    });
    assert!(matches!(events.last(), Some(RunEvent::Failed(_))));
    fake.0.borrow_mut().fail_with = None;
    fake.0
        .borrow_mut()
        .script("catalogue_search", vec![search_payload()]);
    let events = app.run(Workflow::CatalogueSearch {
        query: "x".to_string(),
    });
    assert!(matches!(events.last(), Some(RunEvent::Done(_))));
}

#[test]
fn oversized_bundle_is_refused_before_any_call() {
    let dir = std::env::temp_dir().join("torori-tui-test-bundle-oversized");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    for i in 0..(torori_tui::BUNDLE_FILES_MAX + 1) {
        std::fs::write(dir.join(format!("f{i}")), "").expect("write");
    }
    let fake = SharedFake(Rc::new(RefCell::new(FakeFacade::new())));
    let mut app = app_with(&fake);
    app.mode = Mode::Auto;
    let events = app.run(Workflow::BundleIngest {
        dir: dir.display().to_string(),
    });
    assert!(matches!(events.last(), Some(RunEvent::Failed(_))));
    assert!(
        fake.0.borrow().calls.is_empty(),
        "oversized bundle must be refused before validate/ingest calls"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unclassified_ntranch_material_labels_nsfw_fail_closed() {
    let mut eng = engine();
    let mut dispatcher = AuditOnlyDispatcher::new();
    let event = TriggerEvent::validation_completed(3, "unclassified");
    let fired = eng.evaluate(&event, &mut dispatcher);
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].content_label, "nsfw");
    assert_eq!(fired[0].origin_lane, "ntranch");
    let args = candidate_args(&fired[0], &event);
    assert_eq!(args["content_label"], json!("nsfw"));
}

#[test]
fn sfw_catalogue_lane_labels_sfw_with_reason() {
    let mut eng = engine();
    let mut dispatcher = AuditOnlyDispatcher::new();
    let event = TriggerEvent::validation_completed(2, "sfw-catalogue");
    let fired = eng.evaluate(&event, &mut dispatcher);
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].content_label, "sfw");
    let rule = eng
        .rules()
        .iter()
        .find(|r| r.id == fired[0].rule_id)
        .expect("rule");
    assert!(rule.content_label_reason.contains("SFW catalogue"));
}

#[test]
fn zero_failures_fire_nothing() {
    let mut eng = engine();
    let mut dispatcher = AuditOnlyDispatcher::new();
    let fired = eng.evaluate(
        &TriggerEvent::validation_completed(0, "unclassified"),
        &mut dispatcher,
    );
    assert!(fired.is_empty());
    assert!(dispatcher.dispatched.is_empty());
}

#[test]
fn duplicate_event_is_dropped_by_idempotency() {
    let mut eng = engine();
    let mut dispatcher = AuditOnlyDispatcher::new();
    let event = TriggerEvent::validation_completed(1, "unclassified");
    assert_eq!(eng.evaluate(&event, &mut dispatcher).len(), 1);
    assert!(eng.evaluate(&event, &mut dispatcher).is_empty());
    assert!(eng.audit().iter().any(|line| line.contains("duplicate")));
}

#[test]
fn depth_cap_refuses_triggered_events() {
    let mut eng = engine();
    let mut dispatcher = AuditOnlyDispatcher::new();
    let mut event = TriggerEvent::validation_completed(5, "unclassified");
    event.depth = 1; // produced by a triggered call; max_depth is 1
    assert!(eng.evaluate(&event, &mut dispatcher).is_empty());
    assert!(eng.audit().iter().any(|line| line.contains("depth")));
}

#[test]
fn disabled_rule_never_fires() {
    let mut eng = engine();
    let id = eng.rules()[0].id.clone();
    assert!(eng.set_enabled(&id, false));
    let mut dispatcher = AuditOnlyDispatcher::new();
    let fired = eng.evaluate(
        &TriggerEvent::validation_completed(4, "unclassified"),
        &mut dispatcher,
    );
    assert!(fired.is_empty());
}

#[test]
fn rules_reject_depth_beyond_hard_ceiling_and_bad_labels() {
    let bad_depth = "[[rule]]\nid='x'\norigin_lane='ntranch'\ncontent_label='nsfw'\ncontent_label_reason='r'\non_event='validation.completed'\ntarget_mcp='bloch'\ntarget_tool='lesson_candidate'\nmode='auto'\nmax_depth=3\nenabled=true\n";
    assert!(TriggerEngine::parse(bad_depth).is_err());
    let bad_label = bad_depth
        .replace("max_depth=3", "max_depth=1")
        .replace("'nsfw'", "'unknown'");
    assert!(TriggerEngine::parse(&bad_label).is_err());
}

#[test]
fn voice_job_rejects_bad_slot_and_excess_takes_at_staging() {
    let fake = SharedFake(Rc::new(RefCell::new(FakeFacade::new())));
    let mut app = app_with(&fake);
    assert!(
        app.stage(Workflow::VoiceJob {
            conditioning_json: "{}".to_string(),
            slot_class: "opera".to_string(),
            takes: 1,
        })
        .is_err()
    );
    assert!(
        app.stage(Workflow::VoiceJob {
            conditioning_json: "{}".to_string(),
            slot_class: "whisper".to_string(),
            takes: 99,
        })
        .is_err()
    );
    assert!(app.staged.is_none());
    assert!(fake.0.borrow().calls.is_empty());
}

#[test]
fn oversized_line_input_is_refused() {
    let fake = SharedFake(Rc::new(RefCell::new(FakeFacade::new())));
    let mut app = app_with(&fake);
    let long = "x".repeat(torori_tui::INPUT_LINE_BYTES_MAX + 1);
    let out = app.handle_line(&long);
    assert!(out[0].contains("too long"));
}

#[test]
fn truncate_is_char_boundary_safe() {
    let text = "é".repeat(50);
    let out = panels::truncate(&text, 10);
    assert_eq!(out.chars().count(), 10);
    assert!(out.ends_with('…'));
}

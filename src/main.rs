//! torori — the NTRanch estate sound MCP (stdio).
//!
//! Serves the TORORI catalogue, the OpenNSFW library lane under
//! its licence-state gate, GAP5 pair validation + cue-bundle
//! ingest, and the audio generation lanes (Qwen3-TTS voice,
//! deterministic DSP SFX, Route C comp). Every output is a file
//! plus metrics — this binary contains no audio playback path of
//! any kind (the AUDIO SILENCE RULE, build brief §5).
//!
//! As of its first build (D-113/D-137) this server runs on rmcp,
//! the official Rust MCP SDK, and speaks the modern protocol era
//! only (2026-07-28): sessions open with `server/discover` and
//! every request carries its protocol version in `_meta` — there
//! is no `initialize` handshake. Legacy contact is refused at the
//! stdio front door (see `guard`) with the typed `-32022`
//! unsupported-version error.
//!
//! Usage: `torori [--data <dir>] [--root <dir>]`
//! - `--data`: shipped data tree (catalogue/pairs/voices);
//!   defaults to `$TORORI_DATA_DIR`, an installed `../data`, or
//!   `./data`.
//! - `--root`: writable state tree; defaults to `$TORORI_ROOT` or
//!   `$XDG_DATA_HOME/torori` (`~/.local/share/torori`).

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use rmcp::ServiceExt;
use rmcp::transport::async_rw::AsyncRwTransport;
use tokio::sync::Mutex;
use torori::TororiState;
use torori::guard;
use torori::server::TororiServer;
use torori::state::Paths;

/// Duplex buffer between the era guard and the SDK service.
/// Request lines can reach the 4 MiB line bound; the SDK consumes
/// incrementally, so this only needs to cover response bursts.
const DUPLEX_BUFFER_BYTES: usize = 1024 * 1024;

fn parse_paths(args: &[String]) -> Paths {
    let mut data: Option<PathBuf> = None;
    let mut root: Option<PathBuf> = None;
    let mut argv = args.iter();
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--data" => data = argv.next().map(PathBuf::from),
            "--root" => root = argv.next().map(PathBuf::from),
            other => eprintln!("torori: ignoring unknown arg {other}"),
        }
    }
    Paths::resolve(data, root)
}

/// Wire the pieces together: the guard owns stdio, the SDK
/// service sits behind an in-process duplex, and a pump copies
/// its responses back to stdout through the same lock the guard
/// writes with.
async fn run(paths: Paths) -> std::io::Result<()> {
    let state = TororiState::load(paths)
        .map_err(|e| std::io::Error::other(format!("torori: state load failed: {e}")))?;
    let (client_side, server_side) = tokio::io::duplex(DUPLEX_BUFFER_BYTES);
    let (server_read, server_write) = tokio::io::split(server_side);
    let (client_read, client_write) = tokio::io::split(client_side);
    let stdout: guard::SharedStdout = Arc::new(Mutex::new(tokio::io::stdout()));

    let ordering = guard::Ordering::new();
    let handler = TororiServer::new(state);
    let serve = tokio::spawn(async move {
        let transport = AsyncRwTransport::new_server(server_read, server_write);
        match handler.serve(transport).await {
            Ok(running) => running
                .waiting()
                .await
                .map(|_| ())
                .map_err(|e| std::io::Error::other(e.to_string())),
            // The client closed the connection before selecting a
            // lifecycle (a probe, a spawn check, a client that only
            // ever wanted the refusal): bare EOF is a clean end of
            // session.
            Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => Ok(()),
            Err(e) => Err(std::io::Error::other(e.to_string())),
        }
    });
    let mut pump = tokio::spawn(guard::pump(
        client_read,
        Arc::clone(&stdout),
        Arc::clone(&ordering),
    ));

    // When stdin reaches EOF the guard shuts the duplex write
    // side down, which ends the SDK service; the pump wait is
    // bounded as a backstop so a wedged pump cannot hang exit.
    let guard_result = guard::run(tokio::io::stdin(), client_write, stdout, ordering).await;
    let serve_result = match serve.await {
        Ok(result) => result,
        Err(e) => Err(std::io::Error::other(e.to_string())),
    };
    if tokio::time::timeout(std::time::Duration::from_secs(2), &mut pump)
        .await
        .is_err()
    {
        pump.abort();
    }
    guard_result.and(serve_result)
}

/// Maintenance verb: `torori backfill-voice-pack --tree <dir>
/// [--root <dir>] [--pack <name>]` — walk the staged OpenNSFW
/// voice tree read-only and build the library INDEX into the
/// state root (brief T2). Prints the report as JSON; the tree is
/// never modified.
fn backfill_verb(args: &[String]) -> ExitCode {
    let mut tree: Option<PathBuf> = None;
    let mut pack = "opennsfw-voice".to_string();
    let mut argv = args.iter();
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--tree" => tree = argv.next().map(PathBuf::from),
            "--pack" => {
                if let Some(p) = argv.next() {
                    pack = p.clone();
                }
            }
            _ => {}
        }
    }
    let Some(tree) = tree else {
        eprintln!("torori backfill-voice-pack: --tree <dir> is required");
        return ExitCode::FAILURE;
    };
    let paths = parse_paths(args);
    if let Err(e) = paths.ensure_state_dirs() {
        eprintln!("torori backfill-voice-pack: {e}");
        return ExitCode::FAILURE;
    }
    match torori::library::backfill_voice_pack(&paths, &tree, &pack) {
        Ok((_store, report)) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("torori backfill-voice-pack: {e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("backfill-voice-pack") {
        return backfill_verb(&args[1..]);
    }
    let paths = parse_paths(&args);
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("torori: cannot start runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run(paths)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("torori: serve loop ended: {e}");
            ExitCode::FAILURE
        }
    }
}

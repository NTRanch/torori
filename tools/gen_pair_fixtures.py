#!/usr/bin/env python3
"""Generate data/pairs/*.ron — GAP5 program docs.

Sources: GAP5-SPEC §7.1 (pair.moondeep.depth.t3-motes), §7.2
(pair.moondeep.tandem.service.drop-snap), §7.3
(pair.torori.kit.drop-snap, the canonical snap pair), and the
build brief's T3 catalogue migration batch 1 (the conditioning
family pairs in the torori kit program).

Hashes in the spec's worked records are illustrative
placeholders of correct shape; here every cue hash is
sha256(asset_ref) — deterministic, correct shape, and labelled
fixture-grade. Where the spec's records name siblings only
through a shared form's covers list (the five MoonDeep depth
pairs), the siblings are constructed to the same shape as the
named record. R5 requires a SafewordClass form covering every
pair in each program; where the spec text does not name one,
the form is added and marked R5-required in the file header.
"""

import hashlib
import os

OUT = os.path.join(os.path.dirname(__file__), "..", "data", "pairs")


def h(ref: str) -> str:
    return hashlib.sha256(ref.encode()).hexdigest()


def q(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def cue(kind, ref):
    return (
        f"(kind: {kind}, asset_ref: {q(ref)}, asset_sha256: {q(h(ref))}, "
        f"canonical: true, variants: [])"
    )


def pair(
    pair_id,
    program_id,
    consumer,
    subject_form,
    identity,
    partner,
    trigger_source,
    cue_kind,
    cue_ref,
    summary,
    disclosure,
    install_contexts,
    release,
    reach,
    contexts,
    cue_manifest,
):
    partner_ron = f"Some({q(partner)})" if partner else "None"
    fiction_effect = "None"
    return f"""        (
            pair_id: {q(pair_id)},
            program: (program_id: {q(program_id)}, series: None, consumer_class: {consumer}),
            subject: (form: {subject_form}, identity_ref: {q(identity)}, partner_ref: {partner_ron}, trigger_source: {trigger_source}),
            install: (
                cue: {cue(cue_kind, cue_ref)},
                effect: (summary: {q(summary)}, disclosure_ref: {q(disclosure)}, fiction_effect: {fiction_effect}),
                install_contexts: [{", ".join(q(c) for c in install_contexts)}],
                countermand_allowed: true,
            ),
            release: {release},
            scope: (reach: {reach}, contexts: [{", ".join(q(c) for c in contexts)}]),
            author: (estate_ref: "torori-fixtures", canon_refs: ["GAP5-SPEC.md"]),
            manifest_link: (cue_manifest: {q(cue_manifest)}, release_manifests: []),
        )"""


def shared(release_id, form, asset_refs, covers, budget="Unlimited", cue_ref=None, cue_kind="SpokenWord"):
    cue_ron = f"Some({cue(cue_kind, cue_ref)})" if cue_ref else "None"
    return f"""        (
            release_id: {q(release_id)},
            form: {form},
            cue: {cue_ron},
            asset_refs: [{", ".join(q(a) for a in asset_refs)}],
            covers: [{", ".join(q(c) for c in covers)}],
            except: [],
            budget: {budget},
        )"""


def doc(program_id, pairs, forms, header):
    body = ",\n".join(pairs)
    forms_body = ",\n".join(forms)
    return f"""// {header}
(
    program_id: {q(program_id)},
    pairs: [
{body}
    ],
    shared_releases: [
{forms_body}
    ],
)
"""


def kit():
    fam = [
        # pair_id, cue_kind, cue_ref, summary
        ("pair.torori.kit.drop-snap", "Snap", "audio/kit/canonical-snap.wav",
         "the canonical snap: an immediate drop cue for the kit's conditioning work"),
        ("pair.torori.kit.hypnosis-frame", "SpokenPhrase", "audio/kit/hypnosis-frame.wav",
         "the framing phrase that opens a conditioning session"),
        ("pair.torori.kit.countdown", "SpokenWord", "audio/kit/countdown.wav",
         "the countdown cadence that deepens on each count"),
        ("pair.torori.kit.induction-voice", "SpokenPhrase", "audio/kit/induction-voice.wav",
         "the induction-voice stem: slow, low, measured"),
        ("pair.torori.kit.pleasing-echo", "SpokenWord", "audio/kit/pleasing-echo.wav",
         "the approval echo returned when the subject pleases"),
        ("pair.torori.kit.praise-response", "SpokenWord", "audio/kit/praise-response.wav",
         "the praise marker the subject learns to answer"),
        ("pair.torori.kit.watching-side", "TonePair", "audio/kit/watching-side.wav",
         "the two-tone marker for being watched"),
    ]
    pairs = []
    for pid, kind, ref, summary in fam:
        if pid.endswith("drop-snap"):
            release = (
                "Embedded((cue: Some("
                + cue("SpokenWord", "audio/kit/countermand.wav")
                + "), asset_refs: ["
                + q("audio/kit/countermand.wav")
                + "], covers: [], except: [], budget: Unlimited))"
            )
        else:
            release = 'Shared("release.torori.kit.reset")'
        pairs.append(
            pair(
                pid, "torori.kit", "Fiction", "Single", "kit.subject", None,
                "OwnState", kind, ref, summary, "disclosure.torori.kit",
                ["kit.session"], release, "Program", ["kit.session"], pid,
            )
        )
    # Yuri: the dyadic family member (§7 family), inverse form.
    pairs.append(
        pair(
            "pair.torori.kit.yuri-dyad", "torori.kit", "Fiction", "Dyadic",
            "kit.dyad.a", "kit.dyad.b", "PartnerState", "SpokenWord",
            "audio/kit/yuri-dyad.wav",
            "the paired call the two voices answer in turn",
            "disclosure.torori.kit", ["kit.session"],
            'Shared("release.torori.kit.reset")', "Program", ["kit.session"],
            "pair.torori.kit.yuri-dyad",
        )
    )
    all_ids = [p[0] for p in fam] + ["pair.torori.kit.yuri-dyad"]
    forms = [
        shared(
            "release.torori.kit.reset", "ScopedPurge",
            ["audio/kit/reset.wav"], all_ids,
        ),
        shared(
            "release.torori.kit.stop", "SafewordClass",
            ["audio/kit/stop.wav"], all_ids, cue_ref="audio/kit/stop.wav",
        ),
    ]
    return doc(
        "torori.kit", pairs, forms,
        "GAP5 §7.3 canonical snap pair + the brief T3 batch-1 conditioning family.\n"
        "// Cue hashes are fixture-grade: sha256(asset_ref).\n"
        "// The kit safeword form is R5-required (every pair covered).",
    )


def moondeep_depth():
    names = [
        ("pair.moondeep.depth.t1-echoes", "audio/moondeep/echoes.wav", "the first echo pair"),
        ("pair.moondeep.depth.t2-second-sight", "audio/moondeep/second-sight.wav", "the second-sight pair"),
        ("pair.moondeep.depth.t3-motes", "audio/moondeep/motes.wav",
         "the spoken word that hangs motes of light in the deep (GAP5 §7.1 worked record)"),
        ("pair.moondeep.depth.t4-wielded-bar", "audio/moondeep/wielded-bar.wav", "the wielded-bar pair"),
        ("pair.moondeep.depth.t5-two-hands", "audio/moondeep/two-hands.wav", "the two-hands pair"),
    ]
    pairs = [
        pair(
            pid, "moondeep.depth", "Fiction", "Single", "moondeep.diver", None,
            "OwnState", "SpokenWord", ref, summary, "disclosure.moondeep.depth",
            ["moondeep.descent"], 'Shared("release.moondeep.unraveling")',
            "Program", ["moondeep.descent"], pid,
        )
        for pid, ref, summary in names
    ]
    ids = [n[0] for n in names]
    forms = [
        shared(
            "release.moondeep.unraveling", "ScopedPurge",
            ["audio/moondeep/unraveling.wav"], ids, budget="OncePerCampaign",
        ),
        shared(
            "release.moondeep.safeword", "SafewordClass",
            ["audio/moondeep/safeword.wav"], ids,
            cue_ref="audio/moondeep/safeword.wav",
        ),
    ]
    return doc(
        "moondeep.depth", pairs, forms,
        "GAP5 §7.1 worked records: the five depth pairs under the shared Unraveling.\n"
        "// Cue hashes fixture-grade: sha256(asset_ref). The safeword form is\n"
        "// R5-required construction (the spec text names none for this program).",
    )


def moondeep_tandem():
    pid = "pair.moondeep.tandem.service.drop-snap"
    release = (
        "Embedded((cue: Some("
        + cue("SpokenWord", "audio/moondeep/tandem/wakener.wav")
        + "), asset_refs: ["
        + q("audio/moondeep/tandem/wakener.wav")
        + "], covers: [], except: [], budget: Unlimited))"
    )
    pairs = [
        pair(
            pid, "moondeep.tandem", "Player", "Single", "moondeep.tandem.player",
            None, "OwnState", "Snap", "audio/moondeep/tandem/drop-snap.wav",
            "the service drop-snap (GAP5 §7.2 worked record)",
            "disclosure.moondeep.tandem", ["tandem.session"], release,
            "Session", ["tandem.session"], pid,
        )
    ]
    forms = [
        shared(
            "release.moondeep.tandem.purge", "FullReset",
            ["audio/moondeep/tandem/purge.wav"], [pid],
        ),
        shared(
            "release.moondeep.tandem.stop", "SafewordClass",
            ["audio/moondeep/tandem/stop.wav"], [pid],
            cue_ref="audio/moondeep/tandem/stop.wav",
        ),
    ]
    return doc(
        "moondeep.tandem", pairs, forms,
        "GAP5 §7.2 worked record: the Tandem service drop-snap (Player, Session reach).\n"
        "// Program records: the Tandem Purge (FullReset; its deletion semantics are a\n"
        "// consumer act under R6's Player exception) and the STOP SafewordClass.",
    )


def main():
    os.makedirs(OUT, exist_ok=True)
    for name, text in [
        ("kit.ron", kit()),
        ("moondeep-depth.ron", moondeep_depth()),
        ("moondeep-tandem.ron", moondeep_tandem()),
    ]:
        path = os.path.join(OUT, name)
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(text)
        print("wrote", path)


if __name__ == "__main__":
    main()

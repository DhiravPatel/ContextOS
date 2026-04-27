//! End-to-end integration tests for the engine pipeline.
//! These run through the full optimize() path with real-world-ish inputs.

use contextos_core_engine::{
    ChunkKind, Engine, EngineConfig, InputChunk, OptimizationRequest,
};
use contextos_utils::Language;

fn chunk(id: &str, lang: Language, content: &str) -> InputChunk {
    InputChunk {
        id: id.into(),
        path: Some(format!("{id}.src")),
        language: lang,
        content: content.into(),
        kind: ChunkKind::Code,
        priority: 0,
        skeleton_hint: false,
        community: None,
    }
}

#[test]
fn realistic_ts_project_gets_at_least_40_percent_reduction() {
    let a = r#"
        // Utility: formats a user record for display.
        // Written 2023-06-14 by @alice during the big refactor.
        export function formatUser(user: User): string {
            console.log('formatting user', user.id);
            console.debug('raw user payload', user);
            // trim the display name so we don't leak whitespace
            const name = user.displayName.trim();
            return `${name} <${user.email}>`;
        }
    "#;
    let b = r#"
        /* Duplicate-ish formatter used elsewhere. */
        export function formatUser(user: User): string {
            console.log('formatting user', user.id);
            console.debug('raw user payload', user);
            const name = user.displayName.trim();
            return `${name} <${user.email}>`;
        }
    "#;
    let c = r#"
        export function parsePaymentIntent(body: unknown): PaymentIntent {
            // validates the stripe webhook body
            const raw = body as RawIntent;
            if (!raw.id) throw new Error('missing id');
            return { id: raw.id, amount: raw.amount };
        }
    "#;

    // Cache-aware ordering is on by default; disable it here so we can
    // assert rank-ordered output directly.
    let engine = Engine::new(EngineConfig {
        max_tokens: 100_000,
        enable_cache_order: false,
        ..Default::default()
    });
    let result = engine.optimize(OptimizationRequest {
        chunks: vec![
            chunk("a", Language::TypeScript, a),
            chunk("b", Language::TypeScript, b),
            chunk("c", Language::TypeScript, c),
        ],
        query: Some("parse stripe webhook".into()),
    });

    assert!(
        result.reduction_pct >= 40.0,
        "expected ≥40% reduction on redundant TS, got {:.1}%",
        result.reduction_pct
    );
    // Query-relevant chunk should be first after ranking.
    assert_eq!(result.chunks[0].id, "c");
}

#[test]
fn cache_aware_order_is_stable_across_requests() {
    // Same chunks fed in different orders should produce the same final
    // ordering when cache-aware ordering is on. This is what makes provider
    // prompt caches hit across repeated calls.
    let engine = Engine::new(EngineConfig {
        max_tokens: 100_000,
        enable_dedup: false, // keep all chunks so we can compare order directly
        ..Default::default()
    });
    let cs1 = vec![
        chunk("zebra", Language::Rust, "fn z() {}"),
        chunk("alpha", Language::Rust, "fn a() {}"),
        chunk("middle", Language::Rust, "fn m() {}"),
    ];
    let cs2 = vec![cs1[2].clone(), cs1[0].clone(), cs1[1].clone()];

    let r1 = engine.optimize(OptimizationRequest {
        chunks: cs1,
        query: None,
    });
    let r2 = engine.optimize(OptimizationRequest {
        chunks: cs2,
        query: None,
    });

    let ids1: Vec<&str> = r1.chunks.iter().map(|c| c.id.as_str()).collect();
    let ids2: Vec<&str> = r2.chunks.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids1, ids2);
}

#[test]
fn mmr_keeps_diverse_chunks_under_tight_budget() {
    // Two near-duplicate chunks plus one unique chunk; budget only fits two.
    // MMR with default lambda should prefer the unique chunk over the
    // second duplicate, which a pure rank-ordered greedy would not do.
    let dup = "// dedup-resistant: identical lines below\nlet a = 1;\nlet b = 2;\nlet c = 3;\n";
    let unique = "fn distinct_function_name() -> Result<()> { Ok(()) }";
    let engine = Engine::new(EngineConfig {
        max_tokens: 18,
        enable_dedup: false,
        enable_compress: false,
        enable_skeleton: false,
        enable_cache_order: false,
        ..Default::default()
    });
    let result = engine.optimize(OptimizationRequest {
        chunks: vec![
            chunk("d1", Language::Rust, dup),
            chunk("d2", Language::Rust, dup),
            chunk("u", Language::Rust, unique),
        ],
        query: None,
    });
    let ids: std::collections::HashSet<&str> =
        result.chunks.iter().map(|c| c.id.as_str()).collect();
    assert!(
        ids.contains("u"),
        "MMR must include the unique chunk; got {ids:?}"
    );
}

#[test]
fn pipeline_runs_under_200ms_on_small_input() {
    let engine = Engine::new(EngineConfig::default());
    let chunks = (0..50)
        .map(|i| {
            chunk(
                &format!("c{i}"),
                Language::Rust,
                &format!(
                    "// comment {i}\nfn f{i}() {{ println!(\"{i}\"); let x = {i}; }}\n"
                ),
            )
        })
        .collect();
    let result = engine.optimize(OptimizationRequest { chunks, query: None });
    assert!(
        result.elapsed_ms < 200.0,
        "pipeline took {:.1}ms, budget is 200ms",
        result.elapsed_ms
    );
}

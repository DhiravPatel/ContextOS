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

    let engine = Engine::new(EngineConfig {
        max_tokens: 100_000,
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

import { PIPELINE_STAGES } from "@/lib/constants";

export function Pipeline() {
  return (
    <section
      id="how"
      className="border-y border-line bg-bg-elevated/40 py-24 md:py-32"
    >
      <div className="container-tight">
        <div className="mx-auto max-w-2xl text-center">
          <p className="text-xs font-semibold uppercase tracking-widest text-accent">
            How it works
          </p>
          <h2 className="mt-3 text-balance text-3xl font-semibold tracking-tight md:text-4xl">
            Five stages. Every one provably lossless.
          </h2>
          <p className="mt-4 text-base text-fg-muted">
            Every transformation is verified at the syntax tree. Nothing is
            paraphrased, renamed, or summarised — tokens drop because redundancy
            and irrelevance drop.
          </p>
        </div>

        <ol className="mx-auto mt-14 grid max-w-5xl gap-4 md:grid-cols-5">
          {PIPELINE_STAGES.map((stage) => (
            <li
              key={stage.index}
              className="group relative flex flex-col rounded-2xl border border-line bg-bg p-5 transition-colors hover:border-accent/50"
            >
              <div className="flex items-center justify-between">
                <span className="font-mono text-xs text-fg-subtle">
                  0{stage.index}
                </span>
                <span className="rounded-full border border-line px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider text-fg-muted group-hover:border-accent/40 group-hover:text-accent">
                  {stage.savings}
                </span>
              </div>
              <h3 className="mt-4 text-base font-semibold tracking-tight">
                {stage.title}
              </h3>
              <p className="mt-1 text-xs text-fg-subtle">{stage.subtitle}</p>
              <p className="mt-3 text-sm leading-relaxed text-fg-muted">
                {stage.body}
              </p>
            </li>
          ))}
        </ol>

        <p className="mx-auto mt-10 max-w-2xl text-center text-sm text-fg-subtle">
          Full algorithmic justification (BM25 formulas, MinHash collision math,
          PageRank convergence proofs) lives in{" "}
          <a
            href="https://github.com/DhiravPatel/ContextOS/blob/main/docs/ALGORITHMS.md"
            target="_blank"
            rel="noopener noreferrer"
            className="text-accent transition-colors hover:text-accent-cyan"
          >
            docs/ALGORITHMS.md
          </a>
          .
        </p>
      </div>
    </section>
  );
}

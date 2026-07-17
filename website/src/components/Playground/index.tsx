import type {ReactNode} from 'react';
import {useCallback, useEffect, useRef, useState} from 'react';
import useBaseUrl from '@docusaurus/useBaseUrl';
import BrowserOnly from '@docusaurus/BrowserOnly';

import styles from './styles.module.css';

// The playground runs `ulx-syntax`/`ulx-sema` compiled to WASM, right in
// the browser — the same single-file parse + semantic-analysis fast path
// `ulx-lsp` runs on every keystroke (see `crates/ulx-lsp/src/analysis.rs`).
// It doesn't execute anything: no import resolution, no providers, no
// live LLM calls. See `crates/ulx-wasm` for the Rust side.
const DEFAULT_SOURCE = `judge Fluency(subject: text) -> Verdict {
  rubric: """Is this an accurate, fluent translation of the source?
             Answer Pass, Fail(reason), or Escalate if you cannot tell."""
}

conversation Translate(source: text, target_lang: text) -> text {
  system: """You are a professional translator."""
  user: """Translate to {target_lang}: {source}"""
  assistant -> draft: text

  match judge Fluency(draft) {
    Pass         => draft
    Fail(reason) => retry(2) {
                       user: """Rejected: {reason}. Try again."""
                       assistant -> draft
                     } else escalate(human_approval, reason: reason)
    Escalate     => escalate(human_approval, reason: "judge could not decide")
    Score(_)     => draft
  }
}
`;

type Diagnostic = {
  severity: 'error' | 'warning';
  message: string;
  start_line: number;
  start_col: number;
  end_line: number;
  end_col: number;
};

type CheckFn = (source: string) => Diagnostic[];
type Status = 'loading' | 'ready' | 'error';

function PlaygroundInner(): ReactNode {
  // `web`-target wasm-bindgen output, served as a static asset (not run
  // through webpack's module graph) — see the `webpackIgnore` import
  // below. The generated JS glue locates its sibling `.wasm` binary via
  // `import.meta.url`, so it must be imported by its real URL, not
  // bundled.
  const wasmUrl = useBaseUrl('/wasm/ulx_wasm.js');

  const [source, setSource] = useState(DEFAULT_SOURCE);
  const [diagnostics, setDiagnostics] = useState<Diagnostic[] | null>(null);
  const [status, setStatus] = useState<Status>('loading');
  const [loadError, setLoadError] = useState<string | null>(null);
  const checkRef = useRef<CheckFn | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const mod = await import(/* webpackIgnore: true */ wasmUrl);
        await mod.default();
        if (cancelled) return;
        checkRef.current = mod.check as CheckFn;
        setStatus('ready');
      } catch (err) {
        if (cancelled) return;
        setLoadError(err instanceof Error ? err.message : String(err));
        setStatus('error');
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [wasmUrl]);

  const runCheck = useCallback((text: string) => {
    if (!checkRef.current) return;
    try {
      setDiagnostics(checkRef.current(text));
    } catch (err) {
      setDiagnostics([
        {
          severity: 'error',
          message: `internal error: ${err instanceof Error ? err.message : String(err)}`,
          start_line: 0,
          start_col: 0,
          end_line: 0,
          end_col: 0,
        },
      ]);
    }
  }, []);

  // Run once as soon as the WASM module finishes loading, so the default
  // sample already shows diagnostics (or "No errors") without requiring
  // the user to type anything first.
  useEffect(() => {
    if (status === 'ready') runCheck(source);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status]);

  const onChange = (value: string) => {
    setSource(value);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => runCheck(value), 300);
  };

  return (
    <div className={styles.playground}>
      <div className={styles.editorPane}>
        <textarea
          className={styles.editor}
          value={source}
          onChange={(e) => onChange(e.target.value)}
          spellCheck={false}
          disabled={status !== 'ready'}
          aria-label="Ulexite source code"
        />
      </div>
      <div className={styles.diagnosticsPane}>
        <h3>Diagnostics</h3>
        {status === 'loading' && <p>Loading the Ulexite compiler (WASM)…</p>}
        {status === 'error' && (
          <p className={styles.errorText}>Failed to load the compiler: {loadError}</p>
        )}
        {status === 'ready' && diagnostics !== null && diagnostics.length === 0 && (
          <p className={styles.okText}>No errors.</p>
        )}
        {status === 'ready' && diagnostics !== null && diagnostics.length > 0 && (
          <ul className={styles.diagnosticList}>
            {diagnostics.map((d, i) => (
              <li
                key={`${d.start_line}-${d.start_col}-${i}`}
                className={d.severity === 'error' ? styles.error : styles.warning}>
                <span className={styles.location}>
                  {d.start_line + 1}:{d.start_col + 1}
                </span>{' '}
                <span className={styles.severityTag}>{d.severity}</span> {d.message}
              </li>
            ))}
          </ul>
        )}
        <p className={styles.hint}>
          This runs the real parser and single-file semantic analyzer, compiled
          to WebAssembly — the same checks <code>ulx check</code> and{' '}
          <code>ulx-lsp</code> run. It doesn't resolve imports or execute
          anything against a provider.
        </p>
      </div>
    </div>
  );
}

export default function Playground(): ReactNode {
  return (
    <BrowserOnly fallback={<div>Loading playground…</div>}>
      {() => <PlaygroundInner />}
    </BrowserOnly>
  );
}

import type {ReactNode} from 'react';
import {useEffect, useRef, useState} from 'react';
import useBaseUrl from '@docusaurus/useBaseUrl';

import {chatComplete, loadModel, modelChoices, type ModelChoice, type LoadProgress} from './wllama';
import styles from './styles.module.css';

type Message = {role: string; text: string};

type StepResult =
  | {status: 'done'; value: unknown}
  | {status: 'suspended'; cache_key: string; target: string; reason: string; messages: Message[]}
  | {status: 'error'; message: string};

export type UlxRunHandle = {
  step(): StepResult;
  provideAnswer(cacheKey: string, target: string, text: string): void;
};

export type WasmRunApi = {
  conversationNames(source: string): string[];
  conversationParams(source: string, conversation: string): string[];
  ulxStart(source: string, conversation: string, argsJson: string): UlxRunHandle;
};

type RunPhase =
  | {kind: 'idle'}
  | {kind: 'confirm'}
  | {kind: 'downloading'; progress: LoadProgress | null}
  | {kind: 'running'}
  | {kind: 'unsupported'; message: string}
  | {kind: 'error'; message: string}
  | {kind: 'done'; value: unknown};

// A model this size, run on a phone's much smaller memory budget and much
// slower single-thread CPU path, is a real failure mode worth naming up
// front rather than letting it surface as a confusing hang or crash.
const LIKELY_LOW_MEMORY_DEVICE =
  typeof navigator !== 'undefined' &&
  (/Mobi|Android|iPhone|iPad/i.test(navigator.userAgent) ||
    ('deviceMemory' in navigator && (navigator as {deviceMemory?: number}).deviceMemory! < 4));

function withTimeout<T>(promise: Promise<T>, ms: number, message: string): Promise<T> {
  return Promise.race([
    promise,
    new Promise<T>((_, reject) => {
      setTimeout(() => reject(new Error(message)), ms);
    }),
  ]);
}

/** `Value`'s serde shape is `{kind, value}` (`#[serde(tag = "kind", content
 * = "value")]` in `ulx-runtime`'s `value.rs`) — a `Verdict` nested inside
 * follows plain serde defaults instead (a bare string for a unit variant,
 * `{Fail: "..."}` for a tuple variant). Good-enough, not exhaustive,
 * rendering for the Playground's result line. */
function renderValue(value: unknown): string {
  if (value == null || typeof value !== 'object') return String(value);
  const v = value as {kind?: string; value?: unknown};
  if (v.kind === 'Text' && typeof v.value === 'string') return v.value;
  if (v.kind === 'Int' || v.kind === 'Float') return String(v.value);
  if (v.kind === 'Bool') return String(v.value);
  if (v.kind === 'Verdict') {
    const verdict = v.value;
    if (typeof verdict === 'string') return verdict; // Pass | Escalate
    if (verdict && typeof verdict === 'object') {
      const [tag, inner] = Object.entries(verdict)[0] ?? ['?', undefined];
      return `${tag}${inner !== undefined ? `(${JSON.stringify(inner)})` : ''}`;
    }
  }
  return JSON.stringify(value);
}

export default function RunPanel({
  wasm,
  source,
  canRun,
}: {
  wasm: WasmRunApi;
  source: string;
  canRun: boolean;
}): ReactNode {
  const wllamaWasmUrl = useBaseUrl('/wllama/wllama.wasm');

  const [conversations, setConversations] = useState<string[]>([]);
  const [conversation, setConversation] = useState<string>('');
  const [params, setParams] = useState<string[]>([]);
  const [args, setArgs] = useState<Record<string, string>>({});
  const [model, setModel] = useState<ModelChoice>('qwen2.5-0.5b');
  const [phase, setPhase] = useState<RunPhase>({kind: 'idle'});
  const [transcript, setTranscript] = useState<Message[]>([]);
  const [showValidation, setShowValidation] = useState(false);
  const outputRef = useRef<HTMLDivElement | null>(null);

  // Re-derive the conversation/param list whenever the source changes (or
  // stops type-checking cleanly) — best-effort, so a mid-edit parse error
  // just leaves the previous list showing rather than clearing the form.
  useEffect(() => {
    if (!canRun) return;
    try {
      const names = wasm.conversationNames(source);
      setConversations(names);
      setConversation((prev) => (names.includes(prev) ? prev : names[0] ?? ''));
    } catch {
      // leave the previous list in place
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source, canRun]);

  useEffect(() => {
    if (!conversation) {
      setParams([]);
      return;
    }
    try {
      const names = wasm.conversationParams(source, conversation);
      setParams(names);
      setArgs((prev) => {
        const next: Record<string, string> = {};
        for (const name of names) next[name] = prev[name] ?? '';
        return next;
      });
    } catch {
      setParams([]);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source, conversation]);

  const busy = phase.kind === 'downloading' || phase.kind === 'running';

  // Belt-and-suspenders: anything that throws or rejects without being
  // caught by one of the try/catches below (e.g. an error surfacing from
  // wllama's internal worker, which doesn't always reject a promise this
  // code is awaiting) still lands on a visible error message instead of
  // leaving the UI stuck on "Running…" forever with no feedback — which is
  // indistinguishable, from a visitor's perspective, from "broken." Scoped
  // to `busy` so an unrelated page error elsewhere doesn't hijack this UI.
  useEffect(() => {
    if (!busy) return;
    const onError = (e: ErrorEvent) => {
      setPhase({kind: 'error', message: e.message || 'an unexpected error occurred'});
    };
    const onRejection = (e: PromiseRejectionEvent) => {
      const reason = e.reason;
      setPhase({
        kind: 'error',
        message: reason instanceof Error ? reason.message : String(reason),
      });
    };
    window.addEventListener('error', onError);
    window.addEventListener('unhandledrejection', onRejection);
    return () => {
      window.removeEventListener('error', onError);
      window.removeEventListener('unhandledrejection', onRejection);
    };
  }, [busy]);

  const runLoop = async () => {
    setTranscript([]);
    setPhase({kind: 'running'});
    let run: UlxRunHandle;
    try {
      run = wasm.ulxStart(source, conversation, JSON.stringify(args));
    } catch (err) {
      setPhase({kind: 'error', message: err instanceof Error ? err.message : String(err)});
      return;
    }

    // A hard cap, not a tuning knob: guards against a genuine bug turning
    // this into an infinite loop (each iteration is either a real model
    // call or a completing step, never both) rather than anything a real
    // conversation should ever approach.
    for (let i = 0; i < 50; i++) {
      let result: StepResult;
      try {
        result = run.step();
      } catch (err) {
        setPhase({kind: 'error', message: err instanceof Error ? err.message : String(err)});
        return;
      }
      if (result.status === 'done') {
        setPhase({kind: 'done', value: result.value});
        return;
      }
      if (result.status === 'error') {
        setPhase({kind: 'error', message: result.message});
        return;
      }
      // status === 'suspended'
      if (result.target !== 'chat' && result.target !== 'judge') {
        setPhase({
          kind: 'unsupported',
          message: `This program reached \`escalate(${result.target}, ...)\` — human-in-the-loop escalation isn't supported in the Playground yet. Reason: ${result.reason}`,
        });
        return;
      }
      setTranscript((prev) => [...prev, ...result.messages]);
      let reply: string;
      try {
        // A phone's CPU can be an order of magnitude slower than a
        // desktop's for this — generous, but not unbounded, so a genuinely
        // stuck call surfaces a clear message instead of hanging forever.
        reply = await withTimeout(
          chatComplete(result.messages),
          3 * 60 * 1000,
          'the model took too long to respond (over 3 minutes) — this often means the device doesn\'t have enough memory for the selected model; try the smaller model or a desktop browser',
        );
      } catch (err) {
        setPhase({kind: 'error', message: err instanceof Error ? err.message : String(err)});
        return;
      }
      setTranscript((prev) => [...prev, {role: 'assistant', text: reply}]);
      run.provideAnswer(result.cache_key, result.target, reply);
    }
    setPhase({kind: 'error', message: 'gave up after 50 steps without completing'});
  };

  const missingRequired = params.some((name) => !(args[name] ?? '').trim());

  const onRunClick = () => {
    if (missingRequired) {
      setShowValidation(true);
      return;
    }
    if (phase.kind === 'idle' || phase.kind === 'done' || phase.kind === 'error' || phase.kind === 'unsupported') {
      setPhase({kind: 'confirm'});
    }
  };

  const onConfirmDownload = async () => {
    outputRef.current?.scrollIntoView({behavior: 'smooth', block: 'nearest'});
    setPhase({kind: 'downloading', progress: null});
    try {
      await withTimeout(
        loadModel(model, wllamaWasmUrl, (progress) => setPhase({kind: 'downloading', progress})),
        10 * 60 * 1000,
        'the model download/load took too long (over 10 minutes) — check your connection, or this device may not have enough memory for the selected model',
      );
    } catch (err) {
      setPhase({kind: 'error', message: err instanceof Error ? err.message : String(err)});
      return;
    }
    await runLoop();
  };

  return (
    <>
      <div className={styles.runCard}>
        <h3>Run</h3>
        {!canRun && <p className={styles.hint}>Fix the diagnostics above to enable Run.</p>}
        {canRun && (
          <>
            {conversations.length > 1 && (
              <label className={styles.runField}>
                Conversation
                <select
                  value={conversation}
                  onChange={(e) => setConversation(e.target.value)}
                  disabled={busy}>
                  {conversations.map((name) => (
                    <option key={name} value={name}>
                      {name}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {params.map((name) => {
              const empty = !(args[name] ?? '').trim();
              return (
                <label key={name} className={styles.runField}>
                  {name}
                  {showValidation && empty && <span className={styles.requiredMark}> (required)</span>}
                  <input
                    type="text"
                    value={args[name] ?? ''}
                    disabled={busy}
                    className={showValidation && empty ? styles.fieldError : undefined}
                    onChange={(e) => setArgs((prev) => ({...prev, [name]: e.target.value}))}
                  />
                </label>
              );
            })}
            <label className={styles.runField}>
              Model
              <select
                value={model}
                onChange={(e) => setModel(e.target.value as ModelChoice)}
                disabled={busy}>
                {modelChoices().map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.label}
                  </option>
                ))}
              </select>
            </label>

            <div className={styles.runActions}>
              <button
                type="button"
                className={styles.runButton}
                disabled={!conversation || busy}
                onClick={onRunClick}>
                {busy ? 'Running…' : 'Run'}
              </button>
              {showValidation && missingRequired && (
                <span className={styles.errorText}>Fill in every field above to run.</span>
              )}
            </div>

            {phase.kind === 'confirm' && (
              <div className={styles.runConfirm}>
                <p>
                  Running downloads the selected model (a few hundred MB to ~1&nbsp;GB) once, from
                  Hugging Face's public CDN, then caches it in your browser for next time. The
                  model runs and answers entirely on your machine — nothing about your program or
                  its output is sent anywhere.
                </p>
                {LIKELY_LOW_MEMORY_DEVICE && (
                  <p className={styles.errorText}>
                    This looks like a phone or a memory-limited device — a local model needs
                    significant memory and CPU, and may run very slowly, stall, or crash the tab
                    here. The smaller model and a desktop browser are both more likely to work.
                  </p>
                )}
                <button type="button" onClick={onConfirmDownload}>
                  Download &amp; run
                </button>{' '}
                <button type="button" onClick={() => setPhase({kind: 'idle'})}>
                  Cancel
                </button>
              </div>
            )}

            {phase.kind === 'downloading' && (
              <p className={styles.hint}>
                Loading the model…
                {phase.progress && phase.progress.total > 0
                  ? ` ${Math.round((phase.progress.loaded / phase.progress.total) * 100)}%`
                  : ''}
              </p>
            )}
          </>
        )}
      </div>

      <div className={styles.outputCard} ref={outputRef}>
        <h3>Output</h3>
        {transcript.length === 0 && phase.kind === 'idle' && (
          <p className={styles.hint}>Nothing run yet — hit Run above to see the dialogue here.</p>
        )}
        {transcript.length > 0 && (
          <ul className={styles.transcript}>
            {transcript.map((m, i) => (
              <li key={i}>
                <span className={styles.severityTag}>{m.role}</span> {m.text}
              </li>
            ))}
          </ul>
        )}
        {phase.kind === 'done' && <p className={styles.okText}>Result: {renderValue(phase.value)}</p>}
        {phase.kind === 'unsupported' && <p className={styles.hint}>{phase.message}</p>}
        {phase.kind === 'error' && <p className={styles.errorText}>Error: {phase.message}</p>}
      </div>
    </>
  );
}

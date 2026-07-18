import type {ReactNode} from 'react';
import {useEffect, useState} from 'react';
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
        reply = await chatComplete(result.messages);
      } catch (err) {
        setPhase({kind: 'error', message: err instanceof Error ? err.message : String(err)});
        return;
      }
      setTranscript((prev) => [...prev, {role: 'assistant', text: reply}]);
      run.provideAnswer(result.cache_key, result.target, reply);
    }
    setPhase({kind: 'error', message: 'gave up after 50 steps without completing'});
  };

  const onRunClick = () => {
    if (phase.kind === 'idle' || phase.kind === 'done' || phase.kind === 'error' || phase.kind === 'unsupported') {
      setPhase({kind: 'confirm'});
    }
  };

  const onConfirmDownload = async () => {
    setPhase({kind: 'downloading', progress: null});
    try {
      await loadModel(model, wllamaWasmUrl, (progress) => setPhase({kind: 'downloading', progress}));
    } catch (err) {
      setPhase({kind: 'error', message: err instanceof Error ? err.message : String(err)});
      return;
    }
    await runLoop();
  };

  const busy = phase.kind === 'downloading' || phase.kind === 'running';

  return (
    <div className={styles.runPanel}>
      <h3>Run</h3>
      {!canRun && <p className={styles.hint}>Fix the diagnostics above to enable Run.</p>}
      {canRun && (
        <>
          {conversations.length > 1 && (
            <label className={styles.runField}>
              Conversation
              <select value={conversation} onChange={(e) => setConversation(e.target.value)} disabled={busy}>
                {conversations.map((name) => (
                  <option key={name} value={name}>
                    {name}
                  </option>
                ))}
              </select>
            </label>
          )}
          {params.map((name) => (
            <label key={name} className={styles.runField}>
              {name}
              <input
                type="text"
                value={args[name] ?? ''}
                disabled={busy}
                onChange={(e) => setArgs((prev) => ({...prev, [name]: e.target.value}))}
              />
            </label>
          ))}
          <label className={styles.runField}>
            Model
            <select value={model} onChange={(e) => setModel(e.target.value as ModelChoice)} disabled={busy}>
              {modelChoices().map((m) => (
                <option key={m.id} value={m.id}>
                  {m.label}
                </option>
              ))}
            </select>
          </label>

          <button
            className={styles.runButton}
            disabled={!conversation || busy}
            onClick={onRunClick}>
            {busy ? 'Running…' : 'Run'}
          </button>

          {phase.kind === 'confirm' && (
            <div className={styles.runConfirm}>
              <p>
                Running downloads the selected model (a few hundred MB to ~1&nbsp;GB) once, from
                Hugging Face's public CDN, then caches it in your browser for next time. The model
                runs and answers entirely on your machine — nothing about your program or its
                output is sent anywhere.
              </p>
              <button onClick={onConfirmDownload}>Download &amp; run</button>{' '}
              <button onClick={() => setPhase({kind: 'idle'})}>Cancel</button>
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

          {transcript.length > 0 && (
            <ul className={styles.transcript}>
              {transcript.map((m, i) => (
                <li key={i}>
                  <span className={styles.severityTag}>{m.role}</span> {m.text}
                </li>
              ))}
            </ul>
          )}

          {phase.kind === 'done' && (
            <p className={styles.okText}>Result: {renderValue(phase.value)}</p>
          )}
          {phase.kind === 'unsupported' && <p className={styles.hint}>{phase.message}</p>}
          {phase.kind === 'error' && <p className={styles.errorText}>Error: {phase.message}</p>}
        </>
      )}
    </div>
  );
}

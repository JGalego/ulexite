// Thin wrapper around `@wllama/wllama` (llama.cpp compiled to WASM) for the
// Run panel: a real, local, in-browser model — chosen over a WebGPU-based
// runtime (e.g. WebLLM) specifically for broad compatibility, matching the
// rest of the playground's "runs anywhere, no special hardware" story.
//
// Forced single-thread (`n_threads: 1`): multi-thread wasm needs
// `Cross-Origin-Opener-Policy`/`Cross-Origin-Embedder-Policy` response
// headers to enable `SharedArrayBuffer` — this site is plain GitHub Pages,
// which has no mechanism to set custom response headers at all, so
// multi-thread is simply unavailable here (see wllama's README
// "Limitations"). Slower than a GPU-accelerated runtime, but it works with
// zero hosting changes and no "WebGPU not supported" wall for visitors on
// older browsers/devices.
// Imported from the package's compiled `esm/` entry directly, not the bare
// `@wllama/wllama` specifier — the published package's own `main` field
// points at a root `index.ts` (raw TypeScript source, not compiled JS; a
// packaging bug in the pinned version), which breaks webpack's build if
// anything resolves it via the bare specifier.
import {Wllama} from '@wllama/wllama/esm/index.js';
import type {WllamaChatMessage} from '@wllama/wllama/esm/index.js';

export type ModelChoice = 'qwen2.5-0.5b' | 'qwen2.5-1.5b';

type ModelInfo = {
  label: string;
  repo: string;
  file: string;
  approxSizeMb: number;
};

// Both are the Qwen team's own official GGUF conversions on Hugging Face,
// Q4_K_M-quantized — a reasonable size/quality/speed balance for
// single-threaded in-browser inference (see wllama's README).
const MODELS: Record<ModelChoice, ModelInfo> = {
  'qwen2.5-0.5b': {
    label: 'Qwen2.5 0.5B Instruct — ~380 MB, faster',
    repo: 'Qwen/Qwen2.5-0.5B-Instruct-GGUF',
    file: 'qwen2.5-0.5b-instruct-q4_k_m.gguf',
    approxSizeMb: 380,
  },
  'qwen2.5-1.5b': {
    label: 'Qwen2.5 1.5B Instruct — ~1 GB, more reliable judge verdicts',
    repo: 'Qwen/Qwen2.5-1.5B-Instruct-GGUF',
    file: 'qwen2.5-1.5b-instruct-q4_k_m.gguf',
    approxSizeMb: 1000,
  },
};

export function modelChoices(): Array<{id: ModelChoice} & ModelInfo> {
  return (Object.entries(MODELS) as Array<[ModelChoice, ModelInfo]>).map(([id, info]) => ({
    id,
    ...info,
  }));
}

let wllamaInstance: Wllama | null = null;
let loadedModel: ModelChoice | null = null;

export type LoadProgress = {loaded: number; total: number};

/** No-op if `choice` is already the loaded model — callers can call this
 * unconditionally at the start of a run. */
export async function loadModel(
  choice: ModelChoice,
  wasmUrl: string,
  onProgress: (progress: LoadProgress) => void,
): Promise<void> {
  if (wllamaInstance && loadedModel === choice) return;
  if (wllamaInstance) {
    await wllamaInstance.exit().catch(() => {
      /* best-effort — a fresh Wllama instance below either way */
    });
    wllamaInstance = null;
    loadedModel = null;
  }

  const instance = new Wllama({default: wasmUrl});
  const {repo, file} = MODELS[choice];
  await instance.loadModelFromHF(
    {repo, file},
    {
      progressCallback: onProgress,
      n_threads: 1,
    },
  );
  wllamaInstance = instance;
  loadedModel = choice;
}

/** Ulexite's `chat`/`judge` messages only ever use `system`/`user`/
 * `assistant` roles (see `crates/ulx-ast`'s `MessageRole` and
 * `judge::build_prompt`) — anything else falls back to `user` rather than
 * erroring, since wllama's type only accepts those three. */
function toWllamaRole(role: string): WllamaChatMessage['role'] {
  return role === 'system' || role === 'assistant' ? role : 'user';
}

export async function chatComplete(
  messages: Array<{role: string; text: string}>,
  maxTokens = 512,
): Promise<string> {
  if (!wllamaInstance) {
    throw new Error('no model loaded — call loadModel() first');
  }
  const response = await wllamaInstance.createChatCompletion({
    messages: messages.map((m) => ({role: toWllamaRole(m.role), content: m.text})),
    max_tokens: maxTokens,
    temperature: 0.3,
  });
  return response.choices[0]?.message.content ?? '';
}

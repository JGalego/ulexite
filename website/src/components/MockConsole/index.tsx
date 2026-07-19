import {useEffect, useLayoutEffect, useRef, useState, type ReactNode} from 'react';
import clsx from 'clsx';

import styles from './styles.module.css';

type Tone = 'system' | 'user' | 'assistant' | 'judge' | 'escalate' | 'escalateResolved';

type TurnLine = {
  kind: 'turn';
  emoji: string;
  role: string;
  text: string;
  tone: Tone;
  /** ms to hold before this line appears — bigger for a "thinking" model/judge call, smaller for a line that's effectively already there (e.g. the echoed system prompt). Defaults to `DEFAULT_DELAY_MS`. */
  delayMs?: number;
};

type RuleLine = {kind: 'rule'; delayMs?: number};

type NoteLine = {kind: 'note'; text: string; delayMs?: number};

type SummaryLine = {kind: 'summary'; rows: Array<[string, string]>; delayMs?: number};

export type ConsoleLine = TurnLine | RuleLine | NoteLine | SummaryLine;

export type ConsoleBlock = {
  command: string;
  lines: ConsoleLine[];
};

type MockConsoleProps = {
  blocks: ConsoleBlock[];
  /** ms to hold on the finished, idle prompt before looping back to the first block — like a recorded GIF replaying. `0` disables the loop (plays once and stops). */
  loopPauseMs?: number;
  /** ms to hold after one block's last line before the next command starts typing — the "someone's about to run the next command" beat. */
  interBlockPauseMs?: number;
};

const DEFAULT_DELAY_MS = 650;
const COMMAND_PAUSE_MS = 500;
const INTER_BLOCK_PAUSE_MS = 1500;
const TYPE_CHAR_MS = 22;

// Runs the layout effect before the browser paints on the client (no flash
// of the fully-revealed SSR markup before the replay resets to its start),
// but is a no-op during SSR itself, same as `useEffect` — `useLayoutEffect`
// alone would print React's "does nothing on the server" warning here since
// this component's module also runs during `docusaurus build`'s SSR pass.
const useIsomorphicLayoutEffect = typeof window !== 'undefined' ? useLayoutEffect : useEffect;

/**
 * A hand-authored stand-in for a `vhs`-recorded terminal GIF: same role
 * emoji/coloring `ulx run`'s real `--output text` transcript uses (see
 * `ulx-cli::output::role_style`), sized to its actual content instead of a
 * fixed recording-canvas height, and replayed as a typed command + streamed
 * turns instead of an opaque autoplaying video file. `blocks` plays back as
 * one continuous terminal session — e.g. a `run` that suspends, followed by
 * the `approve` that resumes it — with earlier blocks left in scrollback
 * exactly like a real shell.
 *
 * Renders fully revealed on the server and on the client's first paint (so
 * there's real text for no-JS/SEO, and no SSR/hydration mismatch), then
 * resets and plays the reveal once mounted — skipped entirely for
 * `prefers-reduced-motion: reduce`, which just leaves everything shown.
 *
 * The body's height is locked to that initial fully-revealed measurement
 * (see `minHeight`) before the reset kicks in, so the replay plays out
 * inside a card that's already at its final size from the very first
 * frame — no visible growth as blocks stream in, and no "the demo looks
 * emptier than it will be" gap while it's mid-animation.
 */
export default function MockConsole({
  blocks,
  loopPauseMs = 2600,
  interBlockPauseMs = INTER_BLOCK_PAUSE_MS,
}: MockConsoleProps): ReactNode {
  const lastBlock = blocks.length - 1;
  const [blockIndex, setBlockIndex] = useState(lastBlock);
  const [typedChars, setTypedChars] = useState(blocks[lastBlock].command.length);
  const [visible, setVisible] = useState(blocks[lastBlock].lines.length);
  const [minHeight, setMinHeight] = useState<number | undefined>(undefined);
  const bodyRef = useRef<HTMLDivElement>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  useIsomorphicLayoutEffect(() => {
    if (bodyRef.current) {
      setMinHeight(bodyRef.current.offsetHeight);
    }

    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      return;
    }

    let cancelled = false;
    const after = (ms: number, fn: () => void) => {
      timerRef.current = setTimeout(() => {
        if (!cancelled) fn();
      }, ms);
    };

    function typeCommand(b: number, chars: number) {
      setBlockIndex(b);
      setTypedChars(chars);
      setVisible(0);
      const command = blocks[b].command;
      if (chars < command.length) {
        after(TYPE_CHAR_MS, () => typeCommand(b, chars + 1));
      } else {
        after(COMMAND_PAUSE_MS, () => revealLine(b, 1));
      }
    }

    function revealLine(b: number, step: number) {
      setVisible(step);
      const lines = blocks[b].lines;
      if (step < lines.length) {
        const delay = lines[step]?.delayMs ?? DEFAULT_DELAY_MS;
        after(delay, () => revealLine(b, step + 1));
      } else if (b < lastBlock) {
        after(interBlockPauseMs, () => typeCommand(b + 1, 0));
      } else if (loopPauseMs > 0) {
        after(loopPauseMs, restart);
      }
    }

    function restart() {
      after(COMMAND_PAUSE_MS, () => typeCommand(0, 0));
    }

    restart();

    return () => {
      cancelled = true;
      clearTimeout(timerRef.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [blocks, lastBlock, loopPauseMs, interBlockPauseMs]);

  const activeCommand = blocks[blockIndex].command;
  const activeLines = blocks[blockIndex].lines;
  const typingCommand = typedChars < activeCommand.length;
  const streaming = !typingCommand && visible < activeLines.length;
  const finished = blockIndex === lastBlock && !typingCommand && !streaming;

  return (
    <div className={styles.console}>
      <div className={styles.titlebar}>
        <span className={clsx(styles.dot, styles.dotRed)} />
        <span className={clsx(styles.dot, styles.dotYellow)} />
        <span className={clsx(styles.dot, styles.dotGreen)} />
        <span className={styles.titlebarLabel}>bash</span>
      </div>
      <div ref={bodyRef} className={styles.body} style={minHeight ? {minHeight} : undefined}>
        {blocks.slice(0, blockIndex).map((block, b) => (
          <ConsoleBlockView key={b} command={block.command} lines={block.lines} typedChars={block.command.length} visible={block.lines.length} />
        ))}
        <ConsoleBlockView
          command={activeCommand}
          lines={activeLines}
          typedChars={typedChars}
          visible={typingCommand ? 0 : visible}
        />
        {!typingCommand && streaming && <span className={styles.cursor} aria-hidden="true" />}
        {finished && (
          <div className={styles.line}>
            <span className={styles.prompt}>$ </span>
            <span className={styles.cursor} aria-hidden="true" />
          </div>
        )}
      </div>
    </div>
  );
}

function ConsoleBlockView({
  command,
  lines,
  typedChars,
  visible,
}: {
  command: string;
  lines: ConsoleLine[];
  typedChars: number;
  visible: number;
}): ReactNode {
  const typingCommand = typedChars < command.length;
  return (
    <>
      <div className={styles.line}>
        <span className={styles.prompt}>$ </span>
        <span className={styles.command}>{command.slice(0, typedChars)}</span>
        {typingCommand && <span className={clsx(styles.cursor, styles.cursorSolid)} aria-hidden="true" />}
      </div>
      {!typingCommand && lines.slice(0, visible).map((line, i) => <ConsoleLineView key={i} line={line} />)}
    </>
  );
}

function ConsoleLineView({line}: {line: ConsoleLine}): ReactNode {
  if (line.kind === 'turn') {
    return (
      <div className={clsx(styles.line, styles.turn)}>
        <span className={clsx(styles.role, styles[line.tone])}>
          {line.emoji} {line.role}:
        </span>{' '}
        <span className={styles.turnText}>{line.text}</span>
      </div>
    );
  }
  if (line.kind === 'rule') {
    return <div className={styles.rule} />;
  }
  if (line.kind === 'note') {
    return <div className={clsx(styles.line, styles.note)}>{line.text}</div>;
  }
  return (
    <div className={styles.summary}>
      {line.rows.map(([label, value]) => (
        <div className={styles.summaryRow} key={label}>
          <span className={styles.summaryLabel}>{label}</span>
          <span>{value}</span>
        </div>
      ))}
    </div>
  );
}

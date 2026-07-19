import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import CodeBlock from '@theme/CodeBlock';
import MockConsole, {type ConsoleBlock, type ConsoleLine} from '@site/src/components/MockConsole';
import CrystalMesh from '@site/src/components/CrystalMesh';
import CrystalCursor from '@site/src/components/CrystalCursor';

import styles from './index.module.css';

function HomepageHeader() {
  return (
    <header className={clsx('hero hero--primary', styles.heroBanner)}>
      <CrystalMesh />
      <div className={clsx('container', styles.heroContent)}>
        <Heading as="h1" className="hero__title" data-crystal-target>
          Ulexite
        </Heading>
        <p className="hero__subtitle">
          Stop scripting prompts. Start writing conversations.
        </p>
        <div className={styles.buttons}>
          <Link className="button button--secondary button--lg" to="/docs/getting-started">
            🚀 Get Started
          </Link>
          <Link className="button button--outline button--secondary button--lg" to="/playground">
            🧪 Try the Playground
          </Link>
        </div>
      </div>
    </header>
  );
}

type Feature = {
  title: string;
  description: ReactNode;
};

const FEATURES: Feature[] = [
  {
    title: '💬 Conversation-first',
    description: (
      <>
        History is automatic and structural — every message, in order,
        content-addressed. You never hand-roll a reducer to thread state
        through calls.
      </>
    ),
  },
  {
    title: '🧩 Multimodal artifacts',
    description: (
      <>
        <code>text</code>, <code>image</code>, <code>pdf</code>,{' '}
        <code>audio</code>, <code>embedding</code>, and more — routing a{' '}
        <code>video</code> into a model that only accepts{' '}
        <code>[text, image]</code> is a compile error, not a runtime 400.
      </>
    ),
  },
  {
    title: '⚖️ Built-in judges',
    description: (
      <>
        <code>judge</code> returns a typed, exhaustively-matched{' '}
        <code>Verdict</code> — <code>Pass</code>, <code>Fail(reason)</code>,{' '}
        <code>Score(value)</code>, <code>Escalate</code> — not a hand-written
        grading prompt glued to string matching.
      </>
    ),
  },
  {
    title: '🔁 Traces & replay',
    description: (
      <>
        Every run produces a complete, replayable trace — resume a
        conversation from any point, with model calls memoized instead of
        re-invoked.
      </>
    ),
  },
];

function FeatureGrid() {
  return (
    <section className={styles.features}>
      <div className="container">
        <div className="row">
          {FEATURES.map((feature) => (
            <div key={feature.title} className={clsx('col col--3', styles.featureCol)}>
              <Heading as="h3">{feature.title}</Heading>
              <p>{feature.description}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

const SAMPLE = `judge Fluency(subject: text) -> Verdict {
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
}`;
const RUN_ID = '7f2c9a1e4b0d6f83';

const DEMO_BLOCKS: ConsoleBlock[] = [
  {
    command: 'ulx run translate.ulx Translate --arg source=hello --arg target_lang=fr --provider anthropic',
    lines: [
      {kind: 'turn', emoji: '🧭', role: 'system', tone: 'system', text: 'You are a professional translator.', delayMs: 350},
      {kind: 'turn', emoji: '🧑', role: 'user', tone: 'user', text: 'Translate to fr: hello', delayMs: 400},
      // Longer holds on the two calls that actually go out to a model —
      // `chat` then `judge` — so the reveal reads like real call latency
      // rather than a uniform typewriter tick.
      {kind: 'turn', emoji: '🤖', role: 'assistant', tone: 'assistant', text: 'Bonjour', delayMs: 1100},
      {kind: 'turn', emoji: '⚖️', role: 'judge Fluency', tone: 'judge', text: 'Escalate', delayMs: 900},
      {kind: 'turn', emoji: '🙋', role: 'escalate human_approval', tone: 'escalate', text: 'judge could not decide (suspended)', delayMs: 500},
      {kind: 'note', text: 'suspended: waiting on `human_approval` — judge could not decide', delayMs: 350},
      {kind: 'rule', delayMs: 250},
      {
        kind: 'summary',
        delayMs: 300,
        rows: [
          ['run id', RUN_ID],
          ['status', 'suspended'],
          ['capabilities', 'chat, judge, escalate'],
          ['provider', 'anthropic — chat (claude-haiku-4-5), judge (claude-sonnet-4-5)'],
        ],
      },
      {kind: 'note', text: `resume with: ulx approve ${RUN_ID} --value <text>   (or: ulx deny ${RUN_ID})`},
    ],
  },
  {
    command: `ulx approve ${RUN_ID} --value "Bonjour"`,
    lines: [
      {kind: 'turn', emoji: '🙋', role: 'escalate human_approval', tone: 'escalateResolved', text: 'judge could not decide => Bonjour', delayMs: 500},
      {kind: 'note', text: 'Bonjour', delayMs: 400},
      {kind: 'rule', delayMs: 250},
      {
        kind: 'summary',
        rows: [
          ['run id', RUN_ID],
          ['status', 'ok'],
          ['capabilities', 'chat, judge, escalate'],
          ['provider', 'anthropic — chat (claude-haiku-4-5), judge (claude-sonnet-4-5)'],
        ],
      },
    ],
  },
];

function ShowcaseSection() {
  return (
    <section className={styles.showcase}>
      <div className="container">
        <div className={styles.showcaseGrid}>
          <div>
            <Heading as="h2" className={styles.sectionTitle}>
              👀 What it looks like
            </Heading>
            <p className={styles.sectionSubtitle}>
              A complete, runnable conversation: translate, have a judge
              check fluency, retry once on failure, escalate to a human if
              the judge can't decide.
            </p>
            <CodeBlock language="ulexite" title="translate.ulx">
              {SAMPLE}
            </CodeBlock>
            <p className={styles.sectionSubtitle}>
              Try it yourself in the{' '}
              <Link to="/playground">live playground</Link> — no install
              needed — or see the full{' '}
              <Link to="/docs/examples">examples gallery</Link> for eleven
              more.
            </p>
          </div>
          <div>
            <Heading as="h2" className={styles.sectionTitle}>
              🎬 See it run
            </Heading>
            <p className={styles.sectionSubtitle}>
              <code>ulx run translate.ulx Translate --provider anthropic</code>{' '}
              — the judge can't decide, so it escalates to a human instead
              of guessing. It suspends, a reviewer runs{' '}
              <code>ulx approve</code>, and the same run resumes to
              completion.
            </p>
            <div data-crystal-target>
              <MockConsole blocks={DEMO_BLOCKS} />
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

export default function Home(): ReactNode {
  return (
    <Layout
      title="Ulexite — the conversation-first language for LLM interactions"
      description="A programming language whose primary abstraction is the conversation: typed multimodal artifacts, built-in judges, reproducible traces, and deterministic execution where it counts.">
      <CrystalCursor targetSelector="[data-crystal-target]" />
      <HomepageHeader />
      <main>
        <FeatureGrid />
        <ShowcaseSection />
      </main>
    </Layout>
  );
}

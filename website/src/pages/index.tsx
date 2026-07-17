import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useBaseUrl from '@docusaurus/useBaseUrl';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import CodeBlock from '@theme/CodeBlock';

import styles from './index.module.css';

function HomepageHeader() {
  return (
    <header className={clsx('hero hero--primary', styles.heroBanner)}>
      <div className="container">
        <Heading as="h1" className="hero__title">
          Ulexite
        </Heading>
        <p className="hero__subtitle">
          Stop scripting prompts. Start writing conversations.
        </p>
        <p className={styles.heroBlurb}>
          A language whose primary abstraction is the <strong>conversation</strong> —
          not the prompt, the model, or the agent. Typed multimodal artifacts,
          built-in judges, reproducible traces, and deterministic execution
          where it counts.
        </p>
        <div className={styles.buttons}>
          <Link className="button button--secondary button--lg" to="/docs/getting-started">
            Get Started
          </Link>
          <Link className="button button--outline button--secondary button--lg" to="/playground">
            Try the Playground
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
    title: 'Conversation-first',
    description: (
      <>
        History is automatic and structural — every message, in order,
        content-addressed. You never hand-roll a reducer to thread state
        through calls.
      </>
    ),
  },
  {
    title: 'Typed multimodal artifacts',
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
    title: 'Judges as a language construct',
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
    title: 'Reproducible traces & replay',
    description: (
      <>
        Every run produces a complete, content-addressed, replayable trace by
        default — checkpoint a conversation and resume it from any point,
        with model calls memoized rather than re-invoked.
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

function WhatItLooksLike() {
  return (
    <section className={styles.sample}>
      <div className="container">
        <Heading as="h2" className={styles.sectionTitle}>
          What it looks like
        </Heading>
        <p className={styles.sectionSubtitle}>
          A complete, runnable conversation: translate, have a judge check
          fluency, retry once on failure, escalate to a human if the judge
          can't decide.
        </p>
        <CodeBlock language="ulexite" title="translate.ulx">
          {SAMPLE}
        </CodeBlock>
        <p className={styles.sectionSubtitle}>
          Try it yourself in the{' '}
          <Link to="/playground">live playground</Link> — no install needed —
          or see the full{' '}
          <Link to="/docs/examples">examples gallery</Link> for eleven more.
        </p>
      </div>
    </section>
  );
}

function DemoSection() {
  const gif = useBaseUrl('img/demos/translate.gif');
  return (
    <section className={styles.demo}>
      <div className="container">
        <Heading as="h2" className={styles.sectionTitle}>
          See it run
        </Heading>
        <p className={styles.sectionSubtitle}>
          <code>ulx run translate.ulx Translate</code>, recorded against a
          real provider — not staged.
        </p>
        <img src={gif} alt="Terminal recording of ulx run translate.ulx" className={styles.demoGif} />
      </div>
    </section>
  );
}

function WhyUlexite() {
  return (
    <section className={styles.why}>
      <div className="container">
        <div className={styles.whyBox}>
          <Heading as="h2" className={styles.sectionTitle}>
            Why &ldquo;Ulexite&rdquo;?
          </Heading>
          <p>
            Ulexite is a real mineral, nicknamed the &ldquo;TV rock&rdquo; — it
            grows as a bundle of parallel fibers that pipe an image
            undistorted from one face of the stone to the other. Fitting for
            a language whose job is carrying a conversation faithfully from
            one end to the other.
          </p>
        </div>
      </div>
    </section>
  );
}

function ComparisonTeaser() {
  return (
    <section className={styles.compare}>
      <div className="container">
        <Heading as="h2" className={styles.sectionTitle}>
          How it compares
        </Heading>
        <div className={styles.compareGrid}>
          <p>
            Conversation-first, provider-independent by construction, typed
            artifacts checked at compile time, native traces and replay,
            testing (<code>expect</code>/<code>assert</code>/<code>snapshot</code>)
            as grammar rather than a bolted-on config-file test runner.
          </p>
          <p>
            It's also new — low production track record compared to
            LangGraph's, and the interpreter has a real performance ceiling
            compared to compiled orchestration code. See the full{' '}
            <Link to="/docs/comparison">comparison</Link> and{' '}
            <Link to="/docs/limitations">known limitations</Link> for the
            honest tradeoffs, not just the pitch.
          </p>
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
      <HomepageHeader />
      <main>
        <FeatureGrid />
        <WhatItLooksLike />
        <DemoSection />
        <WhyUlexite />
        <ComparisonTeaser />
      </main>
    </Layout>
  );
}

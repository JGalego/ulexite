import type {ReactNode} from 'react';
import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import Playground from '@site/src/components/Playground';

import styles from './playground.module.css';

export default function PlaygroundPage(): ReactNode {
  return (
    <Layout
      title="Playground"
      description="Try Ulexite's parser, type checker, and a real local model live in your browser, compiled to WebAssembly.">
      <main className={styles.main}>
        <div className="container">
          <Heading as="h1">Playground</Heading>
          <p className={styles.intro}>
            Edit the source on the left — diagnostics from the real{' '}
            <code>ulx-syntax</code> parser and <code>ulx-sema</code> semantic
            analyzer update as you type, compiled to WebAssembly and running
            entirely in your browser. Hit <strong>Run</strong> in the panel on
            the right to actually execute the conversation against a real,
            small local model — no API key, no account, and no data ever
            leaves your browser for a provider call; the only network traffic
            is a one-time, opt-in download of the model file itself (from
            Hugging Face's public CDN), cached afterward. See{' '}
            <Link to="/docs/getting-started">Getting Started</Link> for how
            to run one against a full-size model instead.
          </p>
          <Playground />
        </div>
      </main>
    </Layout>
  );
}

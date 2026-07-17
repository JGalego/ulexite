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
      description="Try Ulexite's parser and type checker live in your browser, compiled to WebAssembly.">
      <main className={styles.main}>
        <div className="container">
          <Heading as="h1">Playground</Heading>
          <p className={styles.intro}>
            Edit the source on the left — diagnostics from the real{' '}
            <code>ulx-syntax</code> parser and <code>ulx-sema</code> semantic
            analyzer update as you type, compiled to WebAssembly and running
            entirely in your browser. Nothing is sent anywhere; there's no
            network call, no provider, and no execution — this is "try the
            compiler," not "run a conversation." See{' '}
            <Link to="/docs/getting-started">Getting Started</Link> for how
            to actually run one.
          </p>
          <Playground />
        </div>
      </main>
    </Layout>
  );
}

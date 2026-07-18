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
            Edit the source and see diagnostics update live, compiled to
            WebAssembly. Hit <strong>Run</strong> to execute it against a
            real local model, entirely in your browser — no API key, no
            account, nothing sent anywhere except a one-time model download.
            See <Link to="/docs/getting-started">Getting Started</Link> to
            run one against a full-size model instead.
          </p>
          <Playground />
        </div>
      </main>
    </Layout>
  );
}

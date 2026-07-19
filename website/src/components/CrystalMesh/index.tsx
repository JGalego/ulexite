import type {ReactNode} from 'react';

import mesh from './mesh.json';
import styles from './styles.module.css';

type Triangle = [points: string, fill: string] | [points: string, fill: string, delay: number, duration: number];

const TRIANGLES = mesh.triangles as Triangle[];

/**
 * A low-poly triangulated "crystal facet" mesh — the logo's own faceted
 * shards, tiled flat — sitting behind the hero content at low opacity so
 * the teal brand color still dominates. A small fraction of facets slowly
 * pulse brighter/dimmer on staggered timers (`.glint`), evoking light
 * catching different faces of the mineral rather than any large-scale
 * motion, which read as distracting in an earlier drifting-gradient
 * version of this background. Static (no animation at all) for
 * `prefers-reduced-motion: reduce` — see `styles.module.css`.
 *
 * The mesh itself (`mesh.json`) is generated once, offline, by a seeded
 * PRNG (see `scripts/gen-crystal-mesh.mjs`'s description in that script —
 * not run at build time, its output is just checked in) rather than
 * computed in the browser, so there's no `Math.random()`/hydration
 * mismatch risk and no runtime cost beyond rendering ~180 static
 * `<polygon>`s.
 */
export default function CrystalMesh(): ReactNode {
  return (
    <svg
      className={styles.mesh}
      viewBox={mesh.viewBox}
      preserveAspectRatio="xMidYMid slice"
      aria-hidden="true"
    >
      {TRIANGLES.map(([points, fill, delay, duration], i) =>
        delay === undefined ? (
          <polygon key={i} points={points} fill={fill} />
        ) : (
          <polygon
            key={i}
            points={points}
            fill={fill}
            className={styles.glint}
            style={{animationDelay: `${delay}s`, animationDuration: `${duration}s`}}
          />
        ),
      )}
    </svg>
  );
}

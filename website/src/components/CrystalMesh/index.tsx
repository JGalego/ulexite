import type {ReactNode} from 'react';

import mesh from './mesh.json';
import styles from './styles.module.css';

type Triangle = [points: string, fill: string];

const TRIANGLES = mesh.triangles as Triangle[];

/**
 * A low-poly triangulated "crystal facet" mesh — the logo's own faceted
 * shards, tiled flat — sitting behind the hero content at low opacity so
 * the teal brand color still dominates. Static: no animation runs on its
 * own. Each facet brightens on hover instead (pure CSS `:hover` on the
 * `<polygon>`s themselves — see `styles.module.css`), so the tessellation
 * responds to the visitor without ever moving on its own.
 *
 * The mesh itself (`mesh.json`) is generated once, offline, by a seeded
 * PRNG (see `scripts/gen-crystal-mesh.mjs` — not run at build time, its
 * output is just checked in) rather than computed in the browser, so
 * there's no `Math.random()`/hydration mismatch risk and no runtime cost
 * beyond rendering ~180 static `<polygon>`s.
 */
export default function CrystalMesh(): ReactNode {
  return (
    <svg
      className={styles.mesh}
      viewBox={mesh.viewBox}
      preserveAspectRatio="xMidYMid slice"
      aria-hidden="true"
    >
      {TRIANGLES.map(([points, fill], i) => (
        <polygon key={i} points={points} fill={fill} />
      ))}
    </svg>
  );
}

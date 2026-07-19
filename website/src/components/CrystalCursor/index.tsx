import {useEffect, type ReactNode} from 'react';

import styles from './styles.module.css';

type Props = {
  /** CSS selector matching every zone that should swap the pointer for the crystal — e.g. `[data-crystal-target]`. */
  targetSelector: string;
};

const LENS_WIDTH = 108;
const LENS_HEIGHT = 68;
const ZOOM = 1.4;

/**
 * Every actual content rect inside `el` — built from a `Range` over its
 * contents rather than `el.getBoundingClientRect()`, since a block element
 * (e.g. a centered `<h1>`) is typically much wider than the glyphs it
 * renders. `getClientRects()` on that range instead yields one tight box
 * per rendered line/run, so hit-testing against them means the crystal
 * only shows up over the actual "Ulexite" ink, not the whole heading's
 * padded-out box.
 */
function contentRects(el: Element): DOMRect[] {
  const range = document.createRange();
  range.selectNodeContents(el);
  return Array.from(range.getClientRects());
}

function pointInRects(x: number, y: number, rects: DOMRect[]): boolean {
  return rects.some((r) => x >= r.left && x <= r.right && y >= r.top && y <= r.bottom);
}

/**
 * Replaces the mouse pointer with a small translucent "crystal" that
 * magnifies whatever's directly behind it — the mineral's own trick,
 * piping an image through undistorted, just enlarged here rather than
 * 1:1. Only over the actual rendered content of any element matching
 * `targetSelector` (see `contentRects` above), and only ever on the
 * landing page this is mounted from, not site-wide.
 *
 * The magnification is a real one, not a color filter: every frame it's
 * active, it clones the hovered target's live DOM, scales the clone with
 * `transform-origin` pinned to the cursor's exact position within it (so
 * that point stays put on screen while everything around it grows), and
 * clips the result to the crystal's pebble shape via `overflow: hidden`
 * on the lens. Re-cloned on every move rather than cached, because the
 * mock console keeps streaming new lines — a cached clone would freeze a
 * stale snapshot instead of tracking the still-animating original.
 *
 * Real-pointer-only: gated on `(hover: hover) and (pointer: fine)`, so it
 * never attaches a listener (and never fires) on touch devices. The one
 * `mousemove` listener is rAF-throttled. Managed with plain DOM APIs
 * inside the effect rather than React state — the lens/clone churn every
 * frame while active, which is a poor fit for React's render cycle for
 * something with no other UI to keep in sync.
 */
export default function CrystalCursor({targetSelector}: Props): ReactNode {
  useEffect(() => {
    if (!window.matchMedia('(hover: hover) and (pointer: fine)').matches) {
      return;
    }

    const lens = document.createElement('div');
    lens.className = styles.lens;
    lens.setAttribute('aria-hidden', 'true');
    lens.style.display = 'none';
    document.body.appendChild(lens);

    let rafId: number | undefined;

    function clear() {
      lens.style.display = 'none';
      lens.replaceChildren();
      document.body.style.cursor = '';
    }

    function handleMove(e: MouseEvent) {
      if (rafId) {
        cancelAnimationFrame(rafId);
      }
      const {clientX, clientY} = e;
      rafId = requestAnimationFrame(() => {
        const targets = Array.from(document.querySelectorAll(targetSelector)).filter(
          (el) => !lens.contains(el),
        );
        const hit = targets.find((el) => pointInRects(clientX, clientY, contentRects(el)));

        if (!hit) {
          clear();
          return;
        }

        document.body.style.cursor = 'none';

        const rect = hit.getBoundingClientRect();
        const lensLeft = clientX - LENS_WIDTH / 2;
        const lensTop = clientY - LENS_HEIGHT / 2;
        lens.style.display = '';
        lens.style.left = `${lensLeft}px`;
        lens.style.top = `${lensTop}px`;

        lens.replaceChildren();
        const clone = hit.cloneNode(true) as HTMLElement;
        clone.removeAttribute('data-crystal-target');
        clone.querySelectorAll('[data-crystal-target]').forEach((n) => n.removeAttribute('data-crystal-target'));
        clone.querySelectorAll('[id]').forEach((n) => n.removeAttribute('id'));
        Object.assign(clone.style, {
          position: 'absolute',
          margin: '0',
          left: `${rect.left - lensLeft}px`,
          top: `${rect.top - lensTop}px`,
          width: `${rect.width}px`,
          height: `${rect.height}px`,
          transformOrigin: `${clientX - rect.left}px ${clientY - rect.top}px`,
          transform: `scale(${ZOOM})`,
          pointerEvents: 'none',
        });
        lens.appendChild(clone);
      });
    }

    window.addEventListener('mousemove', handleMove);
    return () => {
      window.removeEventListener('mousemove', handleMove);
      if (rafId) {
        cancelAnimationFrame(rafId);
      }
      document.body.style.cursor = '';
      lens.remove();
    };
  }, [targetSelector]);

  return null;
}

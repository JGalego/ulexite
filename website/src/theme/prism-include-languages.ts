// Swizzled from @docusaurus/theme-classic's prism-include-languages.ts,
// with one addition at the bottom: registering the custom `ulexite`
// Prism grammar (src/prism-ulexite.ts) — Prism has no built-in language
// for a project-specific DSL, so this is Docusaurus's documented
// mechanism for adding one that isn't in the `prismjs/components`
// package. Everything above the addition is unchanged from the original.
import siteConfig from '@generated/docusaurus.config';
import type * as PrismNamespace from 'prismjs';
import type {Optional} from 'utility-types';

import registerUlexite from '../prism-ulexite';

export default function prismIncludeLanguages(
  PrismObject: typeof PrismNamespace,
): void {
  const {
    themeConfig: {prism},
  } = siteConfig;
  const {additionalLanguages} = prism as {additionalLanguages: string[]};

  const PrismBefore = globalThis.Prism;
  globalThis.Prism = PrismObject;

  additionalLanguages.forEach((lang) => {
    if (lang === 'php') {
      // eslint-disable-next-line global-require
      require('prismjs/components/prism-markup-templating.js');
    }
    // eslint-disable-next-line global-require, import/no-dynamic-require
    require(`prismjs/components/prism-${lang}`);
  });

  registerUlexite(PrismObject);

  delete (globalThis as Optional<typeof globalThis, 'Prism'>).Prism;
  if (typeof PrismBefore !== 'undefined') {
    globalThis.Prism = PrismObject;
  }
}

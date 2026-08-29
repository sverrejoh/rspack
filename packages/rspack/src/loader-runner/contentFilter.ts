/**
 * Content-based loader filter for the JS side of the loader runner.
 *
 * One Rust->JS crossing carries an entire contiguous run of JS
 * loaders: the Normal loop below walks the loader list itself and only
 * yields back to Rust at a `builtin:` loader. The native filter in
 * `rspack_binding_api` therefore only ever sees the head of a run; for
 * every loader below the head the decision has to be taken here, or
 * not at all.
 *
 * Reads the same `RSPACK_LOADER_CONTENT_FILTER` JSON as the native
 * side: `[{"loader": "<request substring>", "include": ["needle"],
 * "includeRegex": ["pattern"]}]`. A gated loader whose current content
 * matches no needle and no pattern is skipped as identity: content,
 * source map and additional data pass through unchanged. Buffers are
 * searched bytewise for literals, without decoding.
 *
 * `loader` also matches against `<request> id=<options.id>`, which
 * distinguishes registrations that share one loader file (rsbuild's
 * `api.transform` gives every transform the same `transformLoader.mjs`
 * request and tells them apart only by its options). A real
 * implementation attaches the filter to the rule use item and needs no
 * such key.
 */

type ContentGate = {
  loader: string;
  include: string[];
  includeBytes: Buffer[];
  includeRegex: RegExp[];
};

let cachedGates: ContentGate[] | null = null;

function gates(): ContentGate[] {
  if (cachedGates !== null) return cachedGates;
  cachedGates = [];
  const raw = process.env.RSPACK_LOADER_CONTENT_FILTER;
  if (!raw) return cachedGates;
  try {
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) {
      for (const item of parsed) {
        const loader = typeof item?.loader === 'string' ? item.loader : '';
        const include = Array.isArray(item?.include)
          ? item.include.filter((m: unknown) => typeof m === 'string')
          : [];
        const includeRegex: RegExp[] = [];
        if (Array.isArray(item?.includeRegex)) {
          for (const pattern of item.includeRegex) {
            if (typeof pattern !== 'string') continue;
            try {
              includeRegex.push(new RegExp(pattern));
            } catch (e) {
              console.error(
                `[rspack] invalid content filter regex ${pattern}: ${e}`,
              );
            }
          }
        }
        if (loader && (include.length || includeRegex.length)) {
          cachedGates.push({
            loader,
            include,
            includeBytes: include.map((m: string) => Buffer.from(m)),
            includeRegex,
          });
        }
      }
    }
  } catch (e) {
    console.error(`[rspack] invalid RSPACK_LOADER_CONTENT_FILTER: ${e}`);
  }
  return cachedGates;
}

let skipped = 0;
let ran = 0;
let statsHookInstalled = false;

function countSkip(didSkip: boolean) {
  if (process.env.RSPACK_LOADER_CONTENT_FILTER_STATS !== '1') return;
  if (didSkip) skipped++;
  else ran++;
  if (!statsHookInstalled) {
    statsHookInstalled = true;
    process.on('exit', () => {
      console.error(
        `[rspack] content-filter(js): skipped=${skipped} ran=${ran}`,
      );
    });
  }
}

/**
 * True when this loader is gated and `content` cannot match, so the
 * loader's normal function can be skipped as identity.
 */
export function contentFilterShouldSkip(
  request: string,
  content: unknown,
  options?: unknown,
): boolean {
  const all = gates();
  if (all.length === 0) return false;
  const id =
    options && typeof options === 'object' && 'id' in options
      ? (options as { id?: unknown }).id
      : undefined;
  const key = typeof id === 'string' ? `${request} id=${id}` : request;
  const gate = all.find((g) => key.includes(g.loader));
  if (!gate) return false;
  let matched: boolean;
  if (typeof content === 'string') {
    matched =
      gate.include.some((m) => content.includes(m)) ||
      gate.includeRegex.some((re) => re.test(content));
  } else if (Buffer.isBuffer(content)) {
    matched = gate.includeBytes.some((m) => content.includes(m));
    if (!matched && gate.includeRegex.length) {
      const text = content.toString('utf8');
      matched = gate.includeRegex.some((re) => re.test(text));
    }
  } else {
    // Unknown content shape: never skip.
    return false;
  }
  countSkip(!matched);
  return !matched;
}

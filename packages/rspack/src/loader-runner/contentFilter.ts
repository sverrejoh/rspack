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
 * side: `[{"loader": "<request substring>", "include": ["needle"]}]`.
 * A gated loader whose current content contains none of the needles is
 * skipped as identity: content, source map and additional data pass
 * through unchanged. Buffers are searched bytewise, without decoding.
 */

type ContentGate = {
	loader: string;
	include: string[];
	includeBytes: Buffer[];
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
				if (loader && include.length) {
					cachedGates.push({
						loader,
						include,
						includeBytes: include.map((m: string) => Buffer.from(m)),
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
 * True when `request` is gated and `content` cannot match, so the
 * loader's normal function can be skipped as identity.
 */
export function contentFilterShouldSkip(
	request: string,
	content: unknown,
): boolean {
	const all = gates();
	if (all.length === 0) return false;
	const gate = all.find((g) => request.includes(g.loader));
	if (!gate) return false;
	let matched: boolean;
	if (typeof content === 'string') {
		matched = gate.include.some((m) => content.includes(m));
	} else if (Buffer.isBuffer(content)) {
		matched = gate.includeBytes.some((m) => content.includes(m));
	} else {
		// Unknown content shape: never skip.
		return false;
	}
	countSkip(!matched);
	return !matched;
}

/**
 * Shared kv scan helper for this example's services.
 *
 * Both the api Worker and the indexer daemon page through the shared `index`
 * kv with the same cursor-following generator. The structural store type
 * accepts both the SDK's and the bindings package's `Kv` handle.
 */

export interface KvScanPage {
  items: { key: string; value: Uint8Array }[]
  nextCursor?: string
}

export interface KvScanStore {
  scan(prefix: string, limit?: number, cursor?: string): Promise<KvScanPage>
}

/** Iterate every key under a prefix, following the scan cursor across pages. */
export async function* scanAll(store: KvScanStore, prefix: string) {
  let cursor: string | undefined
  do {
    const page = await store.scan(prefix, undefined, cursor)
    for (const item of page.items) yield item
    cursor = page.nextCursor
  } while (cursor)
}

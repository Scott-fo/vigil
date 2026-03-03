import fuzzysort from "fuzzysort";

export function searchBranchRefs(
	refs: ReadonlyArray<string>,
	query: string,
): ReadonlyArray<string> {
	const normalizedQuery = query.trim();
	if (normalizedQuery.length === 0) {
		return refs;
	}

	return fuzzysort
		.go(normalizedQuery, refs, {
			limit: refs.length,
		})
		.map((result) => result.target);
}

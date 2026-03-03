export interface RepoChangedEvent {
	readonly subscriptionId: string;
	readonly repoRoot: string;
	readonly version: number;
	readonly changedAt: string;
}

interface DrainedSseBlocks {
	readonly blocks: ReadonlyArray<string>;
	readonly remaining: string;
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null;
}

export function appendSseChunk(buffer: string, chunk: string): string {
	return `${buffer}${chunk.replace(/\r\n/g, "\n")}`;
}

export function drainSseBlocks(buffer: string): DrainedSseBlocks {
	const blocks: Array<string> = [];
	let remaining = buffer;

	while (true) {
		const separatorIndex = remaining.indexOf("\n\n");
		if (separatorIndex < 0) {
			break;
		}

		blocks.push(remaining.slice(0, separatorIndex));
		remaining = remaining.slice(separatorIndex + 2);
	}

	return { blocks, remaining };
}

export function parseRepoChangedEventBlock(
	block: string,
): RepoChangedEvent | null {
	let eventName = "message";
	const dataLines: Array<string> = [];

	for (const rawLine of block.split("\n")) {
		const line = rawLine.trimEnd();
		if (!line || line.startsWith(":")) {
			continue;
		}

		if (line.startsWith("event:")) {
			eventName = line.slice("event:".length).trim();
			continue;
		}

		if (line.startsWith("data:")) {
			dataLines.push(line.slice("data:".length).trimStart());
		}
	}

	if (eventName !== "repo-changed" || dataLines.length === 0) {
		return null;
	}

	let decoded: unknown;
	try {
		decoded = JSON.parse(dataLines.join("\n"));
	} catch {
		return null;
	}

	if (!isRecord(decoded)) {
		return null;
	}

	const subscriptionId = decoded.subscriptionId;
	const repoRoot = decoded.repoRoot;
	const version = decoded.version;
	const changedAt = decoded.changedAt;

	if (
		typeof subscriptionId !== "string" ||
		typeof repoRoot !== "string" ||
		typeof version !== "number" ||
		typeof changedAt !== "string"
	) {
		return null;
	}

	return {
		subscriptionId,
		repoRoot,
		version,
		changedAt,
	};
}

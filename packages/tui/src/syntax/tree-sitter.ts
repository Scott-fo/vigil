import {
	addDefaultParsers,
	getTreeSitterClient,
	pathToFiletype,
	type FiletypeParserOptions,
	type TreeSitterClient,
} from "@opentui/core";
import { Data, Effect, pipe, Schema } from "effect";
import parserConfig from "#config/parsers";

const ParserConfigSchema = Schema.Struct({
	parsers: Schema.optional(Schema.Array(Schema.Unknown)),
});

const InjectionMappingSchema = Schema.Struct({
	nodeTypes: Schema.optional(
		Schema.Record({
			key: Schema.String,
			value: Schema.String,
		}),
	),
	infoStringMap: Schema.optional(
		Schema.Record({
			key: Schema.String,
			value: Schema.String,
		}),
	),
});

const ParserQueriesSchema = Schema.Struct({
	highlights: Schema.Array(Schema.NonEmptyString).pipe(Schema.minItems(1)),
	injections: Schema.optional(Schema.Array(Schema.NonEmptyString)),
});

const ParserEntrySchema = Schema.Struct({
	filetype: Schema.NonEmptyString,
	wasm: Schema.NonEmptyString,
	queries: ParserQueriesSchema,
	injectionMapping: Schema.optional(InjectionMappingSchema),
});

type ParserEntry = Schema.Schema.Type<typeof ParserEntrySchema>;

export class TreeSitterInitializeError extends Data.TaggedError(
	"TreeSitterInitializeError",
)<{
	readonly message: string;
	readonly cause: unknown;
}> {}

let parserOptionsCache: FiletypeParserOptions[] | null = null;
let treeSitterInitPromise: Promise<TreeSitterClient> | null = null;

function normalizeInjectionMapping(
	mapping: ParserEntry["injectionMapping"],
): FiletypeParserOptions["injectionMapping"] | undefined {
	if (!mapping) {
		return undefined;
	}

	const nodeTypes = mapping.nodeTypes ? { ...mapping.nodeTypes } : undefined;
	const infoStringMap = mapping.infoStringMap
		? { ...mapping.infoStringMap }
		: undefined;

	if (!nodeTypes && !infoStringMap) {
		return undefined;
	}

	return {
		...(nodeTypes ? { nodeTypes } : {}),
		...(infoStringMap ? { infoStringMap } : {}),
	};
}

function toFiletypeParserOptions(parser: ParserEntry): FiletypeParserOptions {
	const injections = parser.queries.injections;
	const injectionMapping = normalizeInjectionMapping(parser.injectionMapping);

	return {
		filetype: parser.filetype,
		wasm: parser.wasm,
		queries: {
			highlights: [...parser.queries.highlights],
			...(injections && injections.length > 0
				? { injections: [...injections] }
				: {}),
		},
		...(injectionMapping ? { injectionMapping } : {}),
	};
}

function getParserOptions(): FiletypeParserOptions[] {
	if (parserOptionsCache) {
		return parserOptionsCache;
	}

	const config = Effect.runSync(
		pipe(
			Schema.decodeUnknown(ParserConfigSchema)(parserConfig),
			Effect.match({
				onFailure: () =>
					({
						parsers: [],
					}) as Schema.Schema.Type<typeof ParserConfigSchema>,
				onSuccess: (decoded) => decoded,
			}),
		),
	);
	const list = config.parsers ?? [];

	parserOptionsCache = list
		.map((entry) =>
			Effect.runSync(
				pipe(
					Schema.decodeUnknown(ParserEntrySchema)(entry),
					Effect.match({
						onFailure: () => null,
						onSuccess: (decoded) => toFiletypeParserOptions(decoded),
					}),
				),
			),
		)
		.filter((item): item is FiletypeParserOptions => item !== null);

	return parserOptionsCache;
}

export function initializeTreeSitterClient(): Effect.Effect<
	TreeSitterClient,
	TreeSitterInitializeError
> {
	return Effect.tryPromise({
		try: () => {
			if (!treeSitterInitPromise) {
				addDefaultParsers(getParserOptions());
				const client = getTreeSitterClient();
				treeSitterInitPromise = client.initialize().then(
					() => client,
					(cause) => {
						treeSitterInitPromise = null;
						throw cause;
					},
				);
			}
			return treeSitterInitPromise;
		},
		catch: (cause) =>
			new TreeSitterInitializeError({
				message: "Failed to initialize Tree-sitter client.",
				cause,
			}),
	});
}

export function resolveDiffFiletype(filePath: string): string | undefined {
	const fileName = filePath.toLowerCase().split("/").pop() ?? "";
	if (fileName === "dockerfile") {
		return "dockerfile";
	}

	const openTuiFiletype = pathToFiletype(filePath);
	if (!openTuiFiletype) {
		const ext = fileName.includes(".") ? fileName.split(".").pop() : "";
		if (ext === "diff" || ext === "patch") {
			return "diff";
		}
		return undefined;
	}

	if (
		openTuiFiletype === "typescriptreact" ||
		openTuiFiletype === "javascriptreact" ||
		openTuiFiletype === "javascript"
	) {
		return "typescript";
	}

	if (openTuiFiletype === "shell") {
		return "bash";
	}

	return openTuiFiletype;
}

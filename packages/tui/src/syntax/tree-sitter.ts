import {
	addDefaultParsers,
	type FiletypeParserOptions,
	getTreeSitterClient,
	pathToFiletype,
	type TreeSitterClient,
} from "@opentui/core";
import { Data, Effect, Match, Option, pipe, Schema } from "effect";
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

let parserOptionsCache: Option.Option<ReadonlyArray<FiletypeParserOptions>> =
	Option.none();
let treeSitterInitPromise: Option.Option<Promise<TreeSitterClient>> =
	Option.none();

function decodeParserConfigEntries(): ReadonlyArray<unknown> {
	return Effect.runSync(
		pipe(
			Schema.decodeUnknown(ParserConfigSchema)(parserConfig),
			Effect.map((config) => config.parsers ?? []),
			Effect.orElseSucceed(() => [] as ReadonlyArray<unknown>),
		),
	);
}

function decodeParserEntry(entry: unknown): Option.Option<ParserEntry> {
	return Effect.runSync(
		pipe(Schema.decodeUnknown(ParserEntrySchema)(entry), Effect.option),
	);
}

function normalizeInjectionMapping(
	mapping: ParserEntry["injectionMapping"],
): Option.Option<NonNullable<FiletypeParserOptions["injectionMapping"]>> {
	const nodeTypes = pipe(
		Option.fromNullable(mapping),
		Option.flatMap((resolved) => Option.fromNullable(resolved.nodeTypes)),
	);
	const infoStringMap = pipe(
		Option.fromNullable(mapping),
		Option.flatMap((resolved) => Option.fromNullable(resolved.infoStringMap)),
	);
	if (Option.isNone(nodeTypes) && Option.isNone(infoStringMap)) {
		return Option.none();
	}

	return Option.some({
		...(Option.isSome(nodeTypes) ? { nodeTypes: { ...nodeTypes.value } } : {}),
		...(Option.isSome(infoStringMap)
			? { infoStringMap: { ...infoStringMap.value } }
			: {}),
	});
}

function toFiletypeParserOptions(parser: ParserEntry): FiletypeParserOptions {
	const baseQueries: FiletypeParserOptions["queries"] = {
		highlights: [...parser.queries.highlights],
	};

	const queries = pipe(
		Option.fromNullable(parser.queries.injections),
		Option.filter((injections) => injections.length > 0),
		Option.match({
			onNone: () => baseQueries,
			onSome: (injections) => ({
				...baseQueries,
				injections: [...injections],
			}),
		}),
	);

	const baseParserOptions: FiletypeParserOptions = {
		filetype: parser.filetype,
		wasm: parser.wasm,
		queries,
	};

	return pipe(
		normalizeInjectionMapping(parser.injectionMapping),
		Option.match({
			onNone: () => baseParserOptions,
			onSome: (injectionMapping) => ({
				...baseParserOptions,
				injectionMapping,
			}),
		}),
	);
}

function getParserOptions(): FiletypeParserOptions[] {
	if (Option.isSome(parserOptionsCache)) {
		return [...parserOptionsCache.value];
	}

	const parserOptions: FiletypeParserOptions[] = [];
	for (const entry of decodeParserConfigEntries()) {
		const decoded = decodeParserEntry(entry);
		if (Option.isSome(decoded)) {
			parserOptions.push(toFiletypeParserOptions(decoded.value));
		}
	}

	parserOptionsCache = Option.some(parserOptions);
	return [...parserOptions];
}

export function initializeTreeSitterClient(): Effect.Effect<
	TreeSitterClient,
	TreeSitterInitializeError
> {
	return Effect.tryPromise({
		try: () =>
			pipe(
				treeSitterInitPromise,
				Option.match({
					onSome: (initializingClient) => initializingClient,
					onNone: () => {
						addDefaultParsers(getParserOptions());
						const client = getTreeSitterClient();
						const initializingClient = client.initialize().then(
							() => client,
							(cause) => {
								treeSitterInitPromise = Option.none();
								throw cause;
							},
						);
						treeSitterInitPromise = Option.some(initializingClient);
						return initializingClient;
					},
				}),
			),
		catch: (cause) =>
			new TreeSitterInitializeError({
				message: "Failed to initialize Tree-sitter client.",
				cause,
			}),
	});
}

function extractFileName(filePath: string): string {
	const loweredPath = filePath.toLowerCase();
	const separatorIndex = Math.max(
		loweredPath.lastIndexOf("/"),
		loweredPath.lastIndexOf("\\"),
	);
	return separatorIndex === -1
		? loweredPath
		: loweredPath.slice(separatorIndex + 1);
}

function extractFileExtension(fileName: string): Option.Option<string> {
	const dotIndex = fileName.lastIndexOf(".");
	if (dotIndex <= 0 || dotIndex === fileName.length - 1) {
		return Option.none();
	}
	return Option.some(fileName.slice(dotIndex + 1));
}

function resolveBuiltinFiletype(
	openTuiFiletype: string,
): Option.Option<string> {
	return Match.value(openTuiFiletype).pipe(
		Match.when("typescriptreact", () => Option.some("typescript")),
		Match.when("javascriptreact", () => Option.some("typescript")),
		Match.when("javascript", () => Option.some("typescript")),
		Match.when("shell", () => Option.some("bash")),
		Match.orElse((resolved) => Option.some(resolved)),
	);
}

function resolvePatchExtensionFiletype(
	extension: string,
): Option.Option<string> {
	return Match.value(extension).pipe(
		Match.when("diff", () => Option.some("diff")),
		Match.when("patch", () => Option.some("diff")),
		Match.orElse(() => Option.none()),
	);
}

export function resolveDiffFiletype(filePath: string): Option.Option<string> {
	const fileName = extractFileName(filePath);
	return fileName === "dockerfile"
		? Option.some("dockerfile")
		: pipe(
				Option.fromNullable(pathToFiletype(filePath)),
				Option.flatMap((openTuiFiletype) =>
					resolveBuiltinFiletype(openTuiFiletype),
				),
				Option.orElse(() =>
					pipe(
						extractFileExtension(fileName),
						Option.flatMap((extension) =>
							resolvePatchExtensionFiletype(extension),
						),
					),
				),
			);
}

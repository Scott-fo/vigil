import { Effect, Option, Schema } from "effect";
import type {
	LineThreadAnchor,
	OverallThreadAnchor,
	ReviewScope,
	ThreadAnchor,
} from "./types.ts";

interface WorkingTreeScopeInput {
	readonly repoRoot: string;
	readonly branchOrHead: string;
}

interface BranchCompareScopeInput {
	readonly repoRoot: string;
	readonly sourceRef: string;
	readonly destinationRef: string;
}

export class ReviewScopeValidationError extends Schema.TaggedError<ReviewScopeValidationError>()(
	"ReviewScopeValidationError",
	{
		field: Schema.String,
		message: Schema.String,
	},
) {}

const validateNonEmpty = Effect.fn("ReviewScope.validateNonEmpty")(function* (
	value: string,
	field: string,
) {
	const normalized = value.trim();

	if (normalized.length === 0) {
		return yield* ReviewScopeValidationError.make({
			field,
			message: `${field} must not be empty.`,
		});
	}

	return normalized;
});

function optionToAnchorPart(value: Option.Option<string>): string {
	return Option.match(value, {
		onNone: () => "",
		onSome: (part) => part,
	});
}

function validateLineNumber(
	value: number,
): Effect.Effect<number, ReviewScopeValidationError> {
	return Number.isInteger(value) && value >= 1
		? Effect.succeed(value)
		: Effect.fail(
				ReviewScopeValidationError.make({
					field: "lineNumber",
					message: "lineNumber must be an integer greater than or equal to 1.",
				}),
			);
}

export const buildWorkingTreeScopeKey = Effect.fn(
	"ReviewScope.buildWorkingTreeScopeKey",
)(function* (branchOrHead: string) {
	const normalizedBranch = yield* validateNonEmpty(
		branchOrHead,
		"branchOrHead",
	);
	return `working-tree:${normalizedBranch}`;
});

export const buildBranchCompareScopeKey = Effect.fn(
	"ReviewScope.buildBranchCompareScopeKey",
)(function* (input: {
	readonly sourceRef: string;
	readonly destinationRef: string;
}) {
	const sourceRef = yield* validateNonEmpty(input.sourceRef, "sourceRef");
	const destinationRef = yield* validateNonEmpty(
		input.destinationRef,
		"destinationRef",
	);

	return `branch-compare:${destinationRef}...${sourceRef}`;
});

export const createWorkingTreeScope = Effect.fn(
	"ReviewScope.createWorkingTreeScope",
)(function* (input: WorkingTreeScopeInput) {
	const repoRoot = yield* validateNonEmpty(input.repoRoot, "repoRoot");
	const scopeKey = yield* buildWorkingTreeScopeKey(input.branchOrHead);

	return {
		repoRoot,
		mode: "working-tree" as const,
		sourceRef: Option.none<string>(),
		destinationRef: Option.none<string>(),
		scopeKey,
	} satisfies ReviewScope;
});

export const createBranchCompareScope = Effect.fn(
	"ReviewScope.createBranchCompareScope",
)(function* (input: BranchCompareScopeInput) {
	const repoRoot = yield* validateNonEmpty(input.repoRoot, "repoRoot");
	const sourceRef = yield* validateNonEmpty(input.sourceRef, "sourceRef");
	const destinationRef = yield* validateNonEmpty(
		input.destinationRef,
		"destinationRef",
	);
	const scopeKey = yield* buildBranchCompareScopeKey({
		sourceRef,
		destinationRef,
	});

	return {
		repoRoot,
		mode: "branch-compare" as const,
		sourceRef: Option.some(sourceRef),
		destinationRef: Option.some(destinationRef),
		scopeKey,
	} satisfies ReviewScope;
});

const encodeLineAnchor = Effect.fn("ReviewScope.encodeLineAnchor")(function* (
	anchor: LineThreadAnchor,
) {
	const filePath = yield* validateNonEmpty(anchor.filePath, "filePath");
	const lineNumber = yield* validateLineNumber(anchor.lineNumber);

	return [
		"line",
		filePath,
		anchor.lineSide,
		String(lineNumber),
		optionToAnchorPart(anchor.lineContentHash),
		optionToAnchorPart(anchor.hunkHeader),
	].join("|");
});

export const buildThreadAnchorKey = Effect.fn(
	"ReviewScope.buildThreadAnchorKey",
)(function* (anchor: ThreadAnchor) {
	return anchor.anchorType === "overall"
		? "overall"
		: yield* encodeLineAnchor(anchor);
});

export function createOverallAnchor(): OverallThreadAnchor {
	return {
		anchorType: "overall",
	};
}

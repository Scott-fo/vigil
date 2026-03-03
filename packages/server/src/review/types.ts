import type { Option } from "effect";
import type { ReviewComment } from "../models/review-comment.ts";
import type {
	ReviewLineSide,
	ReviewScopeType,
	ReviewThread,
} from "../models/review-thread.ts";

export interface ReviewScope {
	readonly repoRoot: string;
	readonly mode: ReviewScopeType;
	readonly sourceRef: Option.Option<string>;
	readonly destinationRef: Option.Option<string>;
	readonly scopeKey: string;
}

export interface OverallThreadAnchor {
	readonly anchorType: "overall";
}

export interface LineThreadAnchor {
	readonly anchorType: "line";
	readonly filePath: string;
	readonly lineSide: ReviewLineSide;
	readonly lineNumber: number;
	readonly hunkHeader: Option.Option<string>;
	readonly lineContentHash: Option.Option<string>;
}

export type ThreadAnchor = OverallThreadAnchor | LineThreadAnchor;

export interface ThreadWithComments {
	readonly thread: ReviewThread;
	readonly comments: ReadonlyArray<ReviewComment>;
	readonly isStale: boolean;
}

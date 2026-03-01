import { RGBA } from "@opentui/core";
import { Option } from "effect";
import { memo } from "react";
import type { ResolvedTheme } from "#theme/theme";

export interface CommitModalProps {
	readonly theme: ResolvedTheme;
	readonly modalBackdropColor: RGBA;
	readonly commitMessage: string;
	readonly commitError: Option.Option<string>;
	readonly onCommitMessageChange: (value: string) => void;
	readonly onCommitSubmit: (payload: unknown) => void;
}

export const CommitModal = memo(function CommitModal(props: CommitModalProps) {
	return (
		<box
			position="absolute"
			left={0}
			top={0}
			width="100%"
			height="100%"
			justifyContent="center"
			alignItems="center"
			backgroundColor={props.modalBackdropColor}
			zIndex={100}
		>
			<box
				width={72}
				border
				borderStyle="rounded"
				borderColor={props.theme.borderActive}
				backgroundColor={props.theme.backgroundPanel}
				padding={1}
				flexDirection="column"
			>
				<text fg={props.theme.text}>
					<strong>Commit Staged Changes</strong>
				</text>
				<box marginTop={1}>
					<input
						value={props.commitMessage}
						onChange={props.onCommitMessageChange}
						onSubmit={props.onCommitSubmit}
						placeholder="Enter commit message..."
						focused
						width="100%"
						backgroundColor={props.theme.backgroundElement}
						focusedBackgroundColor={props.theme.backgroundElement}
						textColor={props.theme.text}
						focusedTextColor={props.theme.text}
						placeholderColor={props.theme.textMuted}
					/>
				</box>
				<box marginTop={1}>
					{Option.isSome(props.commitError) ? (
						<text fg={props.theme.error}>{props.commitError.value}</text>
					) : (
						<text fg={props.theme.textMuted}>
							Enter commits. Esc closes without committing.
						</text>
					)}
				</box>
			</box>
		</box>
	);
});

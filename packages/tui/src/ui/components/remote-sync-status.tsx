import { memo } from "react";
import "opentui-spinner/react";
import type { ResolvedTheme } from "#theme/theme";
import type { RemoteSyncState } from "#ui/state";

interface RemoteSyncStatusProps {
	readonly theme: ResolvedTheme;
	readonly state: RemoteSyncState;
}

export const RemoteSyncStatus = memo(function RemoteSyncStatus(
	props: RemoteSyncStatusProps,
) {
	if (props.state._tag !== "running") {
		return null;
	}

	const label =
		props.state.direction === "push"
			? "Pushing to remote..."
			: "Pulling from remote...";

	return (
		<box
			paddingX={1}
			marginBottom={1}
			flexDirection="row"
			alignItems="center"
			gap={1}
		>
			<spinner name="dots" color={props.theme.primary} />
			<text fg={props.theme.textMuted}>{label}</text>
		</box>
	);
});

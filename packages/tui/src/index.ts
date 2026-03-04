export {
	DaemonSupervisorError,
	ensureManagedDaemonAvailable,
} from "./daemon/supervisor.ts";

export {
	startVigilTuiProgram,
	type StartVigilTuiOptions,
	type StartVigilTuiError,
} from "./bootstrap";

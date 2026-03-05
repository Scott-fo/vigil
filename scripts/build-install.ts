#!/usr/bin/env bun

import fsSync from "node:fs";
import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");

if (process.platform !== "linux") {
	throw new Error(
		`build-install is currently Linux-only. Detected platform: ${process.platform}`,
	);
}

const homeDir = process.env.HOME;
if (!homeDir) {
	throw new Error("HOME is not set.");
}

const installRoot =
	process.env.VIGIL_INSTALL_ROOT?.trim() ||
	path.join(homeDir, ".local", "lib", "vigil");
const binDir =
	process.env.VIGIL_BIN_DIR?.trim() || path.join(homeDir, ".local", "bin");
const binaryPath = path.join(installRoot, "vigil");
const wrapperPath = path.join(binDir, "vigil");
const sourceThemesDir = path.join(rootDir, "packages", "tui", "src", "themes");
const installThemesDir = path.join(installRoot, "themes");

async function pathExists(filePath: string): Promise<boolean> {
	try {
		await fs.access(filePath);
		return true;
	} catch {
		return false;
	}
}

async function resolveOpenTuiParserWorkerPath(): Promise<string> {
	const directNodeModulesPath = path.join(
		rootDir,
		"node_modules",
		"@opentui",
		"core",
		"parser.worker.js",
	);
	if (await pathExists(directNodeModulesPath)) {
		return fsSync.realpathSync(directNodeModulesPath);
	}

	const bunStoreDir = path.join(rootDir, "node_modules", ".bun");
	const bunStoreEntries = await fs.readdir(bunStoreDir, { withFileTypes: true });
	const opentuiCoreEntry = bunStoreEntries.find(
		(entry) => entry.isDirectory() && entry.name.startsWith("@opentui+core@"),
	);
	if (!opentuiCoreEntry) {
		throw new Error(
			"Could not locate @opentui/core in node_modules. Run `bun install` first.",
		);
	}

	const parserWorkerPath = path.join(
		bunStoreDir,
		opentuiCoreEntry.name,
		"node_modules",
		"@opentui",
		"core",
		"parser.worker.js",
	);
	if (!(await pathExists(parserWorkerPath))) {
		throw new Error(
			`Could not locate parser.worker.js for @opentui/core at ${parserWorkerPath}.`,
		);
	}

	return fsSync.realpathSync(parserWorkerPath);
}

await fs.mkdir(installRoot, { recursive: true });
await fs.mkdir(binDir, { recursive: true });
await fs.rm(path.join(rootDir, "dist"), { recursive: true, force: true });

console.log("Resolving OpenTUI parser worker...");
const parserWorker = await resolveOpenTuiParserWorkerPath();
const workerRelativePath = path.relative(rootDir, parserWorker).replaceAll(
	"\\",
	"/",
);
const treeSitterWorkerPath = `/$bunfs/root/${workerRelativePath}`;

console.log("Building standalone binary...");
const buildOutput = await Bun.build({
	entrypoints: [path.join(rootDir, "src", "index.tsx"), parserWorker],
	target: "bun",
	compile: {
		outfile: binaryPath,
	},
	define: {
		OTUI_TREE_SITTER_WORKER_PATH: JSON.stringify(treeSitterWorkerPath),
	},
});
if (!buildOutput.success) {
	for (const log of buildOutput.logs) {
		console.error(log);
	}
	throw new Error("bun build failed.");
}

console.log("Copying bundled themes...");
await fs.rm(installThemesDir, { recursive: true, force: true });
await fs.cp(sourceThemesDir, installThemesDir, { recursive: true });

const wrapperScript = `#!/usr/bin/env sh
export VIGIL_SELF_EXECUTABLE="${binaryPath}"
exec "${binaryPath}" "$@"
`;

await fs.writeFile(wrapperPath, wrapperScript);
await fs.chmod(wrapperPath, 0o755);
await fs.chmod(binaryPath, 0o755);

console.log("Installed vigil.");
console.log(`Binary:  ${binaryPath}`);
console.log(`Wrapper: ${wrapperPath}`);
console.log("");
console.log("If `vigil` is not found, add ~/.local/bin to PATH.");

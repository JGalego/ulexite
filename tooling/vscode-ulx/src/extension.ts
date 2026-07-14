// Client half of ulx-lsp (docs/spec/20-ide-integration.md §20.2). Spawns
// the ulx-lsp binary over stdio — the same transport tower-lsp's
// `Server::new(stdin, stdout, socket)` speaks on the server side — and
// wires it to every open `.ulx` document. The binary itself isn't bundled
// here: it's a separate Rust build (`cargo build --release -p ulx-lsp`),
// resolved either on PATH or via the `ulexite.serverPath` setting below.

import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: vscode.ExtensionContext): void {
  const command = vscode.workspace
    .getConfiguration("ulexite")
    .get<string>("serverPath", "ulx-lsp");

  const serverOptions: ServerOptions = {
    command,
    args: [],
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "ulexite" }],
  };

  client = new LanguageClient(
    "ulexite",
    "Ulexite Language Server",
    serverOptions,
    clientOptions,
  );

  client.start();
  context.subscriptions.push({ dispose: () => void client?.stop() });
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}

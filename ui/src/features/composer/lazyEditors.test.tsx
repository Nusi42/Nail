import { readFileSync, readdirSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { screen } from "@testing-library/react";
import { createEnv, renderWithProviders } from "../../test/harness";
import { LazyMarkdownEditor } from "./lazyEditors";

const NAME = "Message";

describe("lazy editors", () => {
  it("shows a status until the editor chunk is loaded", async () => {
    renderWithProviders(
      <LazyMarkdownEditor value="# Hi" label={NAME} onChange={() => {}} />,
      createEnv(),
    );
    expect(screen.getByRole("status")).toHaveTextContent("Loading the editor…");
    expect(await screen.findByRole("textbox", { name: "Message" })).toHaveAttribute(
      "data-part",
      "composer.markdown.content",
    );
    expect(screen.queryByText("Loading the editor…")).toBeNull();
  });

  it("keeps TipTap and CodeMirror out of every statically imported module", () => {
    const src = resolve(process.cwd(), "src");
    const editorFiles = new Set(["HtmlEditor.tsx", "MarkdownEditor.tsx"]);
    const offenders: string[] = [];
    const walk = (dir: string) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const path = join(dir, entry.name);
        if (entry.isDirectory()) {
          if (entry.name !== "generated") walk(path);
          continue;
        }
        if (!/\.tsx?$/.test(entry.name) || /\.test\.tsx?$/.test(entry.name)) continue;
        if (editorFiles.has(entry.name)) continue;
        const text = readFileSync(path, "utf8");
        // Value imports of the editor libraries or of the editor modules.
        const bad =
          /^import\s+(?!type\b)[^;]*from\s+"(@tiptap\/[^"]+|@codemirror\/[^"]+|\.\/(HtmlEditor|MarkdownEditor))"/m;
        if (bad.test(text)) offenders.push(relative(src, path));
      }
    };
    walk(src);
    expect(offenders).toEqual([]);
  });
});

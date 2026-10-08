import { readFileSync } from "node:fs";
import { basename, relative, resolve } from "node:path";
import { screen } from "@testing-library/react";
import { createEnv, Providers, renderWithProviders } from "../../test/harness";
import { LazyMarkdownEditor } from "./lazyEditors";
import { sourceFiles } from "../../test/sourceFiles";

const NAME = "Message";
const OTHER_NAME = "Nachricht";

describe("lazy editors", () => {
  it("shows a status until the editor chunk is loaded", async () => {
    renderWithProviders(
      <LazyMarkdownEditor value="# Hi" label={NAME} onChange={vi.fn()} />,
      createEnv(),
    );
    expect(screen.getByRole("status")).toHaveTextContent("Loading the editor…");
    expect(await screen.findByRole("textbox", { name: "Message" })).toHaveAttribute(
      "data-part",
      "composer.markdown.content",
    );
    expect(screen.queryByText("Loading the editor…")).toBeNull();
  });

  it("gives the editor a new accessible name without recreating it", async () => {
    const env = createEnv();
    const view = renderWithProviders(
      <LazyMarkdownEditor value="# Hi" label={NAME} onChange={vi.fn()} />,
      env,
    );
    const box = await screen.findByRole("textbox", { name: NAME });
    view.rerender(
      <Providers env={env}>
        <LazyMarkdownEditor value="# Hi" label={OTHER_NAME} onChange={vi.fn()} />
      </Providers>,
    );
    expect(screen.getByRole("textbox", { name: OTHER_NAME })).toBe(box);
  });

  it("keeps TipTap and CodeMirror out of every statically imported module", () => {
    const src = resolve(process.cwd(), "src");
    const editorFiles = new Set(["HtmlEditor.tsx", "MarkdownEditor.tsx"]);
    const offenders: string[] = [];
    // Value imports of the editor libraries or of the editor modules.
    const bad =
      /^import\s+(?!type\b)[^;]*from\s+"(@tiptap\/[^"]+|@codemirror\/[^"]+|\.\/(HtmlEditor|MarkdownEditor))"/m;
    for (const path of sourceFiles(src, { include: /\.tsx?$/, exclude: /\.test\.tsx?$/ })) {
      if (editorFiles.has(basename(path))) continue;
      if (bad.test(readFileSync(path, "utf8"))) offenders.push(relative(src, path));
    }
    expect(offenders).toEqual([]);
  });
});

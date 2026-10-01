import "@testing-library/jest-dom/vitest";
import { configure } from "@testing-library/react";

// `findBy…` / `waitFor` give up after 1 s by default; when cargo builds run
// on the same machine, the larger widget suites (PIM calendar, editors)
// need longer. Passing tests are not slowed down.
configure({ asyncUtilTimeout: 5_000 });

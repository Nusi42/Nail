// Tests of scripts/lib/sbom.mjs. Run: node --test scripts/test/
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  cargoSbom,
  cratePurl,
  integrityHash,
  lockChecksums,
  npmPurl,
  npmSbom,
  resolveLocation,
  shippedPackageIds,
} from "../lib/sbom.mjs";

const REGISTRY = "registry+https://github.com/rust-lang/crates.io-index";

/** A small workspace: app → core → serde; app -dev-> testkit; core -build-> cc. */
function metadata() {
  const pkg = (name, version, source, extra = {}) => ({
    id: `${name} ${version}`,
    name,
    version,
    source,
    license: "MIT OR Apache-2.0",
    ...extra,
  });
  const packages = [
    pkg("app", "0.1.0", null, { license: "LicenseRef-Proprietary" }),
    pkg("core", "0.1.0", null, { license: "LicenseRef-Proprietary" }),
    pkg("testkit", "0.1.0", null),
    pkg("serde", "1.0.0", REGISTRY, { repository: "https://github.com/serde-rs/serde" }),
    pkg("cc", "1.2.0", REGISTRY),
    pkg("odd", "0.1.0", REGISTRY, { license: null, license_file: "LICENSE.txt" }),
  ];
  const dep = (pkgId, kind) => ({ pkg: pkgId, dep_kinds: [{ kind, target: null }] });
  return {
    packages,
    workspace_members: ["app 0.1.0", "core 0.1.0", "testkit 0.1.0"],
    resolve: {
      nodes: [
        { id: "app 0.1.0", deps: [dep("core 0.1.0", null), dep("testkit 0.1.0", "dev")] },
        {
          id: "core 0.1.0",
          deps: [dep("serde 1.0.0", null), dep("cc 1.2.0", "build"), dep("odd 0.1.0", null)],
        },
        { id: "testkit 0.1.0", deps: [dep("serde 1.0.0", null)] },
        { id: "serde 1.0.0", deps: [] },
        { id: "cc 1.2.0", deps: [] },
        { id: "odd 0.1.0", deps: [] },
      ],
    },
  };
}

const LOCK = `version = 4

[[package]]
name = "app"
version = "0.1.0"

[[package]]
name = "serde"
version = "1.0.0"
source = "${REGISTRY}"
checksum = "abc123"
dependencies = [
 "x",
]

[[package]]
name = "cc"
version = "1.2.0"
source = "${REGISTRY}"
checksum = "def456"
`;

test("Cargo.lock checksums", () => {
  const sums = lockChecksums(LOCK);
  assert.equal(sums.get("serde@1.0.0"), "abc123");
  assert.equal(sums.get("cc@1.2.0"), "def456");
  assert.equal(sums.has("app@0.1.0"), false);
});

test("purls only for crates.io crates", () => {
  assert.equal(
    cratePurl({ name: "serde", version: "1.0.0", source: REGISTRY }),
    "pkg:cargo/serde@1.0.0",
  );
  assert.equal(cratePurl({ name: "core", version: "0.1.0", source: null }), null);
  assert.equal(npmPurl("@scope/pkg", "1.0.0"), "pkg:npm/%40scope/pkg@1.0.0");
  assert.equal(npmPurl("react", "19.0.0"), "pkg:npm/react@19.0.0");
});

test("only normal dependencies of the roots ship", () => {
  const ids = shippedPackageIds(metadata(), ["app"]);
  assert.deepEqual([...ids].sort(), ["app 0.1.0", "core 0.1.0", "odd 0.1.0", "serde 1.0.0"]);
  assert.throws(() => shippedPackageIds(metadata(), ["serde"]), /not a workspace member/);
});

test("the CycloneDX document", () => {
  const bom = cargoSbom({
    metadata: metadata(),
    lockText: LOCK,
    product: { name: "Nail", version: "0.1.0" },
    roots: ["app"],
    serialNumber: "urn:uuid:00000000-0000-0000-0000-000000000000",
    timestamp: "2026-01-01T00:00:00Z",
  });
  assert.equal(bom.bomFormat, "CycloneDX");
  assert.equal(bom.specVersion, "1.5");
  assert.equal(bom.metadata.component["bom-ref"], "Nail@0.1.0");
  const byName = Object.fromEntries(bom.components.map((c) => [c.name, c]));
  assert.deepEqual(Object.keys(byName).sort(), ["app", "core", "odd", "serde"]);
  assert.deepEqual(byName.serde, {
    type: "library",
    "bom-ref": "pkg:cargo/serde@1.0.0",
    name: "serde",
    version: "1.0.0",
    licenses: [{ expression: "MIT OR Apache-2.0" }],
    purl: "pkg:cargo/serde@1.0.0",
    hashes: [{ alg: "SHA-256", content: "abc123" }],
    externalReferences: [{ type: "vcs", url: "https://github.com/serde-rs/serde" }],
  });
  assert.equal(byName.app.type, "application");
  assert.equal(byName.app.purl, undefined);
  assert.deepEqual(byName.odd.licenses, [{ license: { name: "LicenseRef-file:LICENSE.txt" } }]);
  const deps = Object.fromEntries(bom.dependencies.map((d) => [d.ref, d.dependsOn]));
  assert.deepEqual(deps["Nail@0.1.0"], ["app@0.1.0"]);
  assert.deepEqual(deps["core@0.1.0"], ["pkg:cargo/odd@0.1.0", "pkg:cargo/serde@1.0.0"]);
  assert.deepEqual(deps["app@0.1.0"], ["core@0.1.0"]);
});

test("npm integrity values become hex hashes", () => {
  assert.deepEqual(integrityHash("sha512-AAEC"), { alg: "SHA-512", content: "000102" });
  assert.equal(integrityHash("md5-xx"), null);
  assert.equal(integrityHash(undefined), null);
});

const NPM_LOCK = {
  lockfileVersion: 3,
  packages: {
    "": { name: "root", workspaces: ["ui"], devDependencies: { prettier: "^3" } },
    ui: {
      name: "@nail/ui",
      version: "0.1.0",
      dependencies: { react: "^19", "@scope/lib": "^1" },
      devDependencies: { vitest: "^4" },
    },
    "node_modules/@nail/ui": { resolved: "ui", link: true },
    "node_modules/react": { version: "19.0.0", license: "MIT", integrity: "sha512-AAEC" },
    "node_modules/@scope/lib": {
      version: "1.0.0",
      license: "ISC",
      dependencies: { react: "^18", tslib: "^2" },
      optionalDependencies: { "native-only": "^1" },
    },
    "node_modules/@scope/lib/node_modules/react": { version: "18.0.0", license: "MIT" },
    "node_modules/tslib": { version: "2.0.0", license: "0BSD" },
    "node_modules/vitest": { version: "4.0.0", dev: true },
    "node_modules/prettier": { version: "3.0.0", dev: true },
  },
};

test("node resolution walks up the node_modules chain", () => {
  const p = NPM_LOCK.packages;
  assert.equal(resolveLocation(p, "ui", "react"), "node_modules/react");
  assert.equal(
    resolveLocation(p, "node_modules/@scope/lib", "react"),
    "node_modules/@scope/lib/node_modules/react",
  );
  assert.equal(resolveLocation(p, "node_modules/@scope/lib", "tslib"), "node_modules/tslib");
  assert.equal(resolveLocation(p, "ui", "missing"), null);
});

test("the UI SBOM has the runtime packages only", () => {
  const bom = npmSbom({
    lock: NPM_LOCK,
    workspace: "ui",
    product: { name: "Nail UI", version: "0.1.0" },
    serialNumber: "urn:uuid:1",
    timestamp: "2026-01-01T00:00:00Z",
  });
  assert.deepEqual(
    bom.components.map((c) => c["bom-ref"]),
    [
      "pkg:npm/%40scope/lib@1.0.0",
      "pkg:npm/react@18.0.0",
      "pkg:npm/react@19.0.0",
      "pkg:npm/tslib@2.0.0",
    ],
  );
  const react = bom.components.find((c) => c.version === "19.0.0");
  assert.deepEqual(react.hashes, [{ alg: "SHA-512", content: "000102" }]);
  assert.deepEqual(react.licenses, [{ expression: "MIT" }]);
  assert.equal(bom.dependencies[0].ref, "Nail UI@0.1.0");
  assert.deepEqual(bom.dependencies[0].dependsOn, [
    "pkg:npm/%40scope/lib@1.0.0",
    "pkg:npm/react@19.0.0",
  ]);
  const lib = bom.dependencies.find((d) => d.ref === "pkg:npm/%40scope/lib@1.0.0");
  assert.deepEqual(lib.dependsOn, ["pkg:npm/react@18.0.0", "pkg:npm/tslib@2.0.0"]);
  assert.throws(() => npmSbom({ ...NPM_LOCK, lock: NPM_LOCK, workspace: "nope" }), /not in/);
});

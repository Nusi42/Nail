/**
 * CycloneDX 1.5 SBOM of the Rust code that ships (plan M2b WP44).
 *
 * Built from `cargo metadata` (the resolved graph for the release target)
 * and `Cargo.lock` (crates.io SHA-256 checksums): every crate reachable
 * from the shipped binaries through normal (not dev, not build)
 * dependencies. Written by hand instead of `cargo cyclonedx`, which writes
 * its files next to each Cargo.toml, i.e. into the repository.
 * The UI's npm packages come from `npm sbom` (scripts/sbom.mjs).
 */

/**
 * A package of `cargo metadata --format-version 1` (the fields used here).
 *
 * @typedef {object} CargoPackage
 * @property {string} id
 * @property {string} name
 * @property {string} version
 * @property {string | null} source
 * @property {string | null} [license]
 * @property {string | null} [license_file]
 * @property {string | null} [description]
 * @property {string | null} [repository]
 */

/**
 * An edge of the resolve graph of `cargo metadata`.
 *
 * @typedef {{ pkg: string, dep_kinds: { kind: string | null }[] }} CargoDep
 */

/**
 * A node of the resolve graph of `cargo metadata`.
 *
 * @typedef {{ id: string, deps: CargoDep[] }} CargoNode
 */

/**
 * `cargo metadata --format-version 1` output (the fields used here).
 *
 * @typedef {object} CargoMetadata
 * @property {CargoPackage[]} packages
 * @property {string[]} workspace_members
 * @property {{ nodes: CargoNode[] }} resolve
 */

/**
 * An entry of package-lock.json v3 `packages`.
 *
 * @typedef {object} LockEntry
 * @property {string} [name]
 * @property {string} [version]
 * @property {string} [license]
 * @property {string} [integrity]
 * @property {Record<string, string>} [dependencies]
 * @property {Record<string, string>} [optionalDependencies]
 * @property {Record<string, string>} [peerDependencies]
 * @property {Record<string, string>} [devDependencies]
 * @property {string[]} [workspaces]
 * @property {string} [resolved]
 * @property {boolean} [link]
 * @property {boolean} [dev]
 */

/** @typedef {{ expression: string } | { license: { name: string } }} License */

/**
 * A CycloneDX component.
 *
 * @typedef {object} Component
 * @property {string} type
 * @property {string} bom-ref
 * @property {string} name
 * @property {string} version
 * @property {string} [description]
 * @property {License[]} [licenses]
 * @property {string} [purl]
 * @property {{ alg: string, content: string }[]} [hashes]
 * @property {{ type: string, url: string }[]} [externalReferences]
 */

/**
 * A CycloneDX 1.5 document (the parts written here).
 *
 * @typedef {object} Bom
 * @property {string} bomFormat
 * @property {string} specVersion
 * @property {string} serialNumber
 * @property {number} version
 * @property {{ timestamp: string,
 *   tools: { components: { type: string, name: string, version: string }[] },
 *   component: Component }} metadata
 * @property {Component[]} components
 * @property {{ ref: string, dependsOn: string[] }[]} dependencies
 */

/** @typedef {{ name: string, version: string }} Product */

/**
 * `name@version` → checksum, from Cargo.lock text.
 *
 * @param {string} lockText
 * @returns {Map<string, string>}
 */
export function lockChecksums(lockText) {
  /** @type {Map<string, string>} */
  const checksums = new Map();
  for (const block of lockText.split(/^\[\[package\]\]\s*$/m).slice(1)) {
    /** @param {string} name */
    const field = (name) => new RegExp(`^${name} = "([^"]*)"`, "m").exec(block)?.[1];
    const name = field("name");
    const version = field("version");
    const checksum = field("checksum");
    if (name && version && checksum) checksums.set(`${name}@${version}`, checksum);
  }
  return checksums;
}

/**
 * Package URL of a crates.io crate; `null` for local / git crates.
 *
 * @param {Pick<CargoPackage, "name" | "version" | "source">} pkg
 * @returns {string | null}
 */
export function cratePurl(pkg) {
  if (!pkg.source?.startsWith("registry+")) return null;
  return `pkg:cargo/${encodeURIComponent(pkg.name)}@${encodeURIComponent(pkg.version)}`;
}

/**
 * @param {CargoPackage} pkg
 * @returns {License[]}
 */
function licenses(pkg) {
  if (pkg.license) return [{ expression: pkg.license }];
  if (pkg.license_file) return [{ license: { name: `LicenseRef-file:${pkg.license_file}` } }];
  return [];
}

/**
 * Whether a resolve-graph edge is a normal (not dev, not build) dependency.
 *
 * @param {CargoDep} dep
 */
function isNormal(dep) {
  return dep.dep_kinds.some((k) => k.kind === null);
}

/**
 * Package ids reachable from `roots` (package names) through normal
 * dependencies, in the resolve graph of `metadata`.
 *
 * @param {CargoMetadata} metadata
 * @param {string[]} roots
 * @returns {Set<string>}
 */
export function shippedPackageIds(metadata, roots) {
  const nodes = new Map(metadata.resolve.nodes.map((n) => [n.id, n]));
  const byName = new Map(
    metadata.packages
      .filter((p) => metadata.workspace_members.includes(p.id))
      .map((p) => [p.name, p.id]),
  );
  /** @type {Set<string>} */
  const seen = new Set();
  const stack = roots.map((name) => {
    const id = byName.get(name);
    if (!id) throw new Error(`${name} is not a workspace member`);
    return id;
  });
  for (let id = stack.pop(); id !== undefined; id = stack.pop()) {
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) {
      if (isNormal(dep)) stack.push(dep.pkg);
    }
  }
  return seen;
}

/**
 * The CycloneDX envelope and `metadata` block shared by both documents.
 *
 * @param {{ product: Product, serialNumber: string, timestamp: string }} options
 * @param {Component[]} components
 * @param {Bom["dependencies"]} dependencies
 * @returns {Bom}
 */
function bom({ product, serialNumber, timestamp }, components, dependencies) {
  return {
    bomFormat: "CycloneDX",
    specVersion: "1.5",
    serialNumber,
    version: 1,
    metadata: {
      timestamp,
      tools: { components: [{ type: "application", name: "nail-sbom", version: "1" }] },
      component: {
        type: "application",
        "bom-ref": `${product.name}@${product.version}`,
        name: product.name,
        version: product.version,
      },
    },
    components,
    dependencies,
  };
}

/**
 * The CycloneDX document. `product`: `{ name, version }` of the whole
 * application; `roots`: the binaries' package names; `serialNumber`,
 * `timestamp`: passed in so the output is reproducible in tests.
 *
 * @param {{ metadata: CargoMetadata, lockText: string, product: Product, roots: string[],
 *   serialNumber: string, timestamp: string }} options
 * @returns {Bom}
 */
export function cargoSbom({ metadata, lockText, product, roots, serialNumber, timestamp }) {
  const checksums = lockChecksums(lockText);
  const ids = shippedPackageIds(metadata, roots);
  const packages = metadata.packages.filter((p) => ids.has(p.id));
  const refOf = new Map(packages.map((p) => [p.id, cratePurl(p) ?? `${p.name}@${p.version}`]));
  /** @param {string} id */
  const ref = (id) => {
    const value = refOf.get(id);
    if (value === undefined) throw new Error(`${id} is not a shipped package`);
    return value;
  };
  const productRef = `${product.name}@${product.version}`;

  const components = packages
    .map((p) => {
      const purl = cratePurl(p);
      /** @type {Component} */
      const component = {
        type: metadata.workspace_members.includes(p.id) ? "application" : "library",
        "bom-ref": ref(p.id),
        name: p.name,
        version: p.version,
      };
      if (p.description) component.description = p.description.trim();
      const license = licenses(p);
      if (license.length) component.licenses = license;
      if (purl) component.purl = purl;
      const checksum = checksums.get(`${p.name}@${p.version}`);
      if (checksum && purl) component.hashes = [{ alg: "SHA-256", content: checksum }];
      if (p.repository) {
        component.externalReferences = [{ type: "vcs", url: p.repository }];
      }
      return component;
    })
    .sort((a, b) => a["bom-ref"].localeCompare(b["bom-ref"]));

  const nodes = new Map(metadata.resolve.nodes.map((n) => [n.id, n]));
  const dependencies = packages
    .map((p) => ({
      ref: ref(p.id),
      dependsOn: (nodes.get(p.id)?.deps ?? [])
        .filter((d) => isNormal(d) && refOf.has(d.pkg))
        .map((d) => ref(d.pkg))
        .sort(),
    }))
    .sort((a, b) => a.ref.localeCompare(b.ref));
  const rootRefs = roots.map((name) => {
    const pkg = packages.find((p) => p.name === name && ids.has(p.id));
    if (!pkg) throw new Error(`${name} is not a shipped package`);
    return ref(pkg.id);
  });
  dependencies.unshift({ ref: productRef, dependsOn: rootRefs.sort() });

  return bom({ product, serialNumber, timestamp }, components, dependencies);
}

/**
 * Package URL of an npm package (`@scope/name` → `%40scope/name`).
 *
 * @param {string} name
 * @param {string} version
 */
export function npmPurl(name, version) {
  const encoded = name.startsWith("@")
    ? `%40${name.slice(1).split("/").map(encodeURIComponent).join("/")}`
    : encodeURIComponent(name);
  return `pkg:npm/${encoded}@${encodeURIComponent(version)}`;
}

/** @type {Partial<Record<string, string>>} */
const HASH_ALGORITHMS = { sha512: "SHA-512", sha384: "SHA-384", sha256: "SHA-256", sha1: "SHA-1" };

/**
 * An npm `integrity` value (`sha512-<base64>`) as a CycloneDX hash.
 *
 * @param {string | undefined} integrity
 * @returns {{ alg: string, content: string } | null}
 */
export function integrityHash(integrity) {
  const match = /^(sha512|sha384|sha256|sha1)-(.+)$/.exec(integrity ?? "");
  const [, name = "", content = ""] = match ?? [];
  const alg = HASH_ALGORITHMS[name];
  if (alg === undefined) return null;
  return { alg, content: Buffer.from(content, "base64").toString("hex") };
}

/**
 * Where Node would find `name` when required from the package at
 * `location` (a package-lock v3 key such as `ui` or `node_modules/a`).
 *
 * @param {Partial<Record<string, LockEntry>>} packages
 * @param {string} location
 * @param {string} name
 * @returns {string | null}
 */
export function resolveLocation(packages, location, name) {
  let dir = location;
  for (;;) {
    const candidate = dir ? `${dir}/node_modules/${name}` : `node_modules/${name}`;
    if (packages[candidate]) return candidate;
    if (!dir) return null;
    const parent = dir.lastIndexOf("/node_modules/");
    dir = parent >= 0 ? dir.slice(0, parent) : "";
  }
}

/**
 * CycloneDX 1.5 document of the npm packages a workspace ships: everything
 * reachable from its `dependencies` (and their optional / peer
 * dependencies that are installed), from package-lock.json (v3).
 *
 * @param {{ lock: { packages: Partial<Record<string, LockEntry>> }, workspace: string,
 *   product: Product, serialNumber: string, timestamp: string }} options
 * @returns {Bom}
 */
export function npmSbom({ lock, workspace, product, serialNumber, timestamp }) {
  const packages = lock.packages;
  const root = packages[workspace];
  if (!root) throw new Error(`${workspace} is not in package-lock.json`);
  /** @param {string} location */
  const entryAt = (location) => {
    const entry = packages[location];
    if (!entry) throw new Error(`${location} is not in package-lock.json`);
    return entry;
  };
  /** @type {Map<string, string[]>} location → locations of its dependencies */
  const edges = new Map();
  const stack = [workspace];
  for (let location = stack.pop(); location !== undefined; location = stack.pop()) {
    if (edges.has(location)) continue;
    const entry = entryAt(location);
    const names = Object.keys({
      ...entry.dependencies,
      ...entry.optionalDependencies,
      ...entry.peerDependencies,
    });
    const deps = [];
    for (const name of names) {
      const target = resolveLocation(packages, location, name);
      if (!target) continue; // optional and not installed on this platform
      deps.push(target);
      stack.push(target);
    }
    edges.set(location, deps);
  }
  edges.delete(workspace);
  /** @param {string} location */
  const nameAt = (location) =>
    entryAt(location).name ?? location.slice(location.lastIndexOf("node_modules/") + 13);
  /** @param {string} location */
  const versionAt = (location) => entryAt(location).version ?? "";
  /** @type {Map<string, string>} location → bom-ref */
  const seen = new Map();
  for (const location of edges.keys()) {
    seen.set(location, npmPurl(nameAt(location), versionAt(location)));
  }
  /** @param {string} location */
  const refAt = (location) => seen.get(location) ?? location;
  const productRef = `${product.name}@${product.version}`;
  const components = [...edges.keys()]
    .map((location) => {
      const entry = entryAt(location);
      /** @type {Component} */
      const component = {
        type: "library",
        "bom-ref": refAt(location),
        name: nameAt(location),
        version: versionAt(location),
        purl: refAt(location),
      };
      if (entry.license) component.licenses = [{ expression: entry.license }];
      const hash = integrityHash(entry.integrity);
      if (hash) component.hashes = [hash];
      return component;
    })
    // The same package@version can be installed at several locations.
    .filter((c, i, all) => all.findIndex((o) => o["bom-ref"] === c["bom-ref"]) === i)
    .sort((a, b) => a["bom-ref"].localeCompare(b["bom-ref"]));
  /** @type {Map<string, string[]>} */
  const dependencies = new Map();
  dependencies.set(productRef, [
    ...new Set(
      Object.keys({ ...root.dependencies, ...root.optionalDependencies })
        .map((name) => resolveLocation(packages, workspace, name))
        .filter((l) => l !== null)
        .map(refAt),
    ),
  ]);
  for (const [location, deps] of edges) {
    const ref = refAt(location);
    const merged = new Set([...(dependencies.get(ref) ?? []), ...deps.map(refAt)]);
    dependencies.set(ref, [...merged]);
  }
  return bom(
    { product, serialNumber, timestamp },
    components,
    [...dependencies]
      .map(([ref, dependsOn]) => ({ ref, dependsOn: dependsOn.sort() }))
      .sort((a, b) =>
        a.ref === productRef ? -1 : b.ref === productRef ? 1 : a.ref.localeCompare(b.ref),
      ),
  );
}

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

/** `name@version` → checksum, from Cargo.lock text. */
export function lockChecksums(lockText) {
  const checksums = new Map();
  for (const block of lockText.split(/^\[\[package\]\]\s*$/m).slice(1)) {
    const field = (name) => block.match(new RegExp(`^${name} = "([^"]*)"`, "m"))?.[1];
    const name = field("name");
    const version = field("version");
    const checksum = field("checksum");
    if (name && version && checksum) checksums.set(`${name}@${version}`, checksum);
  }
  return checksums;
}

/** Package URL of a crates.io crate; `null` for local / git crates. */
export function cratePurl(pkg) {
  if (!pkg.source?.startsWith("registry+")) return null;
  return `pkg:cargo/${encodeURIComponent(pkg.name)}@${encodeURIComponent(pkg.version)}`;
}

function licenses(pkg) {
  if (pkg.license) return [{ expression: pkg.license }];
  if (pkg.license_file) return [{ license: { name: `LicenseRef-file:${pkg.license_file}` } }];
  return [];
}

/**
 * Package ids reachable from `roots` (package names) through normal
 * dependencies, in the resolve graph of `metadata`.
 */
export function shippedPackageIds(metadata, roots) {
  const nodes = new Map(metadata.resolve.nodes.map((n) => [n.id, n]));
  const byName = new Map(
    metadata.packages
      .filter((p) => metadata.workspace_members.includes(p.id))
      .map((p) => [p.name, p.id]),
  );
  const seen = new Set();
  const stack = roots.map((name) => {
    const id = byName.get(name);
    if (!id) throw new Error(`${name} is not a workspace member`);
    return id;
  });
  while (stack.length) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) {
      if (dep.dep_kinds.some((k) => k.kind === null)) stack.push(dep.pkg);
    }
  }
  return seen;
}

/**
 * The CycloneDX document. `product`: `{ name, version }` of the whole
 * application; `roots`: the binaries' package names; `serialNumber`,
 * `timestamp`: passed in so the output is reproducible in tests.
 */
export function cargoSbom({ metadata, lockText, product, roots, serialNumber, timestamp }) {
  const checksums = lockChecksums(lockText);
  const ids = shippedPackageIds(metadata, roots);
  const packages = metadata.packages.filter((p) => ids.has(p.id));
  const refOf = new Map(packages.map((p) => [p.id, cratePurl(p) ?? `${p.name}@${p.version}`]));
  const productRef = `${product.name}@${product.version}`;

  const components = packages
    .map((p) => {
      const purl = cratePurl(p);
      const component = {
        type: metadata.workspace_members.includes(p.id) ? "application" : "library",
        "bom-ref": refOf.get(p.id),
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
      ref: refOf.get(p.id),
      dependsOn: (nodes.get(p.id)?.deps ?? [])
        .filter((d) => d.dep_kinds.some((k) => k.kind === null) && refOf.has(d.pkg))
        .map((d) => refOf.get(d.pkg))
        .sort(),
    }))
    .sort((a, b) => a.ref.localeCompare(b.ref));
  const rootRefs = roots.map((name) =>
    refOf.get(packages.find((p) => p.name === name && ids.has(p.id)).id),
  );
  dependencies.unshift({ ref: productRef, dependsOn: rootRefs.sort() });

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
        "bom-ref": productRef,
        name: product.name,
        version: product.version,
      },
    },
    components,
    dependencies,
  };
}

/** Package URL of an npm package (`@scope/name` → `%40scope/name`). */
export function npmPurl(name, version) {
  const encoded = name.startsWith("@")
    ? `%40${name.slice(1).split("/").map(encodeURIComponent).join("/")}`
    : encodeURIComponent(name);
  return `pkg:npm/${encoded}@${encodeURIComponent(version)}`;
}

/** An npm `integrity` value (`sha512-<base64>`) as a CycloneDX hash. */
export function integrityHash(integrity) {
  const match = /^(sha512|sha384|sha256|sha1)-(.+)$/.exec(integrity ?? "");
  if (!match) return null;
  const alg = { sha512: "SHA-512", sha384: "SHA-384", sha256: "SHA-256", sha1: "SHA-1" }[match[1]];
  return { alg, content: Buffer.from(match[2], "base64").toString("hex") };
}

/**
 * Where Node would find `name` when required from the package at
 * `location` (a package-lock v3 key such as `ui` or `node_modules/a`).
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
 */
export function npmSbom({ lock, workspace, product, serialNumber, timestamp }) {
  const packages = lock.packages;
  const root = packages[workspace];
  if (!root) throw new Error(`${workspace} is not in package-lock.json`);
  const seen = new Map(); // location → bom-ref
  const edges = new Map();
  const stack = [workspace];
  while (stack.length) {
    const location = stack.pop();
    if (edges.has(location)) continue;
    const entry = packages[location];
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
  const nameAt = (location) =>
    packages[location].name ?? location.slice(location.lastIndexOf("node_modules/") + 13);
  for (const location of edges.keys()) {
    seen.set(location, npmPurl(nameAt(location), packages[location].version));
  }
  const productRef = `${product.name}@${product.version}`;
  const components = [...edges.keys()]
    .map((location) => {
      const entry = packages[location];
      const component = {
        type: "library",
        "bom-ref": seen.get(location),
        name: nameAt(location),
        version: entry.version,
        purl: seen.get(location),
      };
      if (entry.license) component.licenses = [{ expression: entry.license }];
      const hash = integrityHash(entry.integrity);
      if (hash) component.hashes = [hash];
      return component;
    })
    // The same package@version can be installed at several locations.
    .filter((c, i, all) => all.findIndex((o) => o["bom-ref"] === c["bom-ref"]) === i)
    .sort((a, b) => a["bom-ref"].localeCompare(b["bom-ref"]));
  const dependencies = new Map();
  dependencies.set(productRef, [
    ...new Set(
      Object.keys({ ...root.dependencies, ...root.optionalDependencies })
        .map((name) => resolveLocation(packages, workspace, name))
        .filter(Boolean)
        .map((l) => seen.get(l)),
    ),
  ]);
  for (const [location, deps] of edges) {
    const ref = seen.get(location);
    const merged = new Set([...(dependencies.get(ref) ?? []), ...deps.map((d) => seen.get(d))]);
    dependencies.set(ref, [...merged]);
  }
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
        "bom-ref": productRef,
        name: product.name,
        version: product.version,
      },
    },
    components,
    dependencies: [...dependencies]
      .map(([ref, dependsOn]) => ({ ref, dependsOn: dependsOn.sort() }))
      .sort((a, b) =>
        a.ref === productRef ? -1 : b.ref === productRef ? 1 : a.ref.localeCompare(b.ref),
      ),
  };
}

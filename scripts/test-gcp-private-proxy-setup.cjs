// Real GCP setup qualification: node scripts/test-gcp-private-proxy-setup.cjs
// Uses test credentials from .env.test and always attempts destruction after apply.
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const assert = require("node:assert/strict");
const repoRoot = path.resolve(__dirname, "..");
process.loadEnvFile(path.join(repoRoot, ".env.test"));
// Run after regenerating the GCP network fixture with the Terraform generator tests.
const credentials = process.env.GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY;
assert(credentials, "test management credentials required");
const project = JSON.parse(credentials).project_id;
const root = fs.mkdtempSync(
  path.join(require("node:os").tmpdir(), "private-proxy-setup-"),
);
fs.chmodSync(root, 0o700);
const prefix =
  "proxy-setup-" + require("node:crypto").randomBytes(4).toString("hex");
const snapshot = fs.readFileSync(
  path.join(
    repoRoot,
    "crates/alien-terraform/tests/generator/snapshots/generator__generator__helpers__gcp_network_create.snap",
  ),
  "utf8",
);
const sections = snapshot.split(/^=== (.+) ===\n/m);
for (let i = 1; i < sections.length; i += 2) {
  if (
    [
      "versions.tf",
      "variables.tf",
      "providers.tf",
      "resource_prefix.tf",
      "locals.tf",
      "default_network.tf",
    ].includes(sections[i])
  )
    fs.writeFileSync(path.join(root, sections[i]), sections[i + 1]);
}
fs.writeFileSync(
  path.join(root, "probe.tf"),
  `output "proxy" { value = { link = google_compute_subnetwork.default_network_proxy[0].self_link, cidr = google_compute_subnetwork.default_network_proxy[0].ip_cidr_range, purpose = google_compute_subnetwork.default_network_proxy[0].purpose, role = google_compute_subnetwork.default_network_proxy[0].role } }\n`,
);
fs.writeFileSync(
  path.join(root, "terraform.tfvars.json"),
  JSON.stringify({
    name: prefix,
    token: "test-only",
    resource_prefix: prefix,
    gcp_project: project,
    gcp_region: "us-central1",
    gcp_proxy_subnet_cidr: "10.45.0.0/23",
  }),
  { mode: 0o600 },
);
const env = {
  ...process.env,
  GOOGLE_CREDENTIALS: credentials,
  TF_IN_AUTOMATION: "1",
};
function tf(args, capture = false) {
  const result = spawnSync("terraform", args, {
    cwd: root,
    env,
    encoding: "utf8",
    stdio: capture ? ["ignore", "pipe", "inherit"] : "inherit",
  });
  assert.equal(
    result.status,
    0,
    `terraform ${args[0]} failed; state retained at ${root}`,
  );
  return result.stdout;
}
console.log(
  `Testing generated proxy subnet setup: ${prefix}; recovery directory: ${root}`,
);
tf(["init", "-input=false", "-no-color"]);
try {
  tf([
    "apply",
    "-auto-approve",
    "-input=false",
    "-no-color",
    "-target=google_compute_subnetwork.default_network_proxy",
  ]);
  const proxy = JSON.parse(tf(["output", "-json", "proxy"], true));
  assert.equal(proxy.purpose, "REGIONAL_MANAGED_PROXY");
  assert.equal(proxy.role, "ACTIVE");
  assert.equal(proxy.cidr, "10.45.0.0/23");
  assert(proxy.link.endsWith(`/subnetworks/${prefix}-default-network-proxy`));
  console.log(
    "Generated setup created an ACTIVE regional proxy subnet with the requested CIDR.",
  );
} finally {
  tf(["destroy", "-auto-approve", "-input=false", "-no-color"]);
  const state = JSON.parse(tf(["show", "-json"], true));
  assert.equal(
    state.values?.root_module?.resources?.length || 0,
    0,
    "all test resources must be gone",
  );
  console.log("Generated setup cleanup verified: empty Terraform state.");
}

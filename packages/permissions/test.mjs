import assert from "node:assert/strict"
import { execFileSync } from "node:child_process"
import { readFileSync } from "node:fs"
import { resolve } from "node:path"
import { test } from "node:test"
import { fileURLToPath } from "node:url"
import { executeOperationPermissions } from "./index.js"

const root = fileURLToPath(new URL("../../", import.meta.url))
const statements = [
  {
    effect: "Allow",
    actions: ["s3:GetObject"],
    resources: ["arn:aws:s3:::*/*"],
    condition: null,
    reasons: ["Read selected objects"],
    sources: [],
  },
]
const catalog = JSON.parse(
  readFileSync(resolve(root, "crates/alien-permissions/src/operations/catalog.json"), "utf8"),
)
const requests = [
  { task: "resolveAwsReferences", references: Object.keys(catalog.aws) },
  {
    task: "resolveAwsReferences",
    references: ["operations/s3/head-object", "operations/ec2/instances"],
  },
  { task: "resolveAwsReferences", references: ["operations/unknown/admin"] },
  {
    task: "compileAws",
    statements,
    ceilings: { s3BucketArns: ["arn:aws:s3:::example-bucket"], sqsQueueArns: [] },
  },
  { task: "compileAws", statements, ceilings: { s3BucketArns: [], sqsQueueArns: [] } },
  {
    task: "compileGcp",
    grants: [
      {
        permission: "storage.objects.list",
        scope: `projects/\${projectName}/buckets/\${resourceName}`,
        sources: [],
      },
    ],
    ceilings: { gcsBucketNames: ["example-bucket"] },
  },
  {
    task: "resolveKubernetes",
    references: ["pods/get"],
    declaredPermissions: null,
    tier: "read-only",
  },
  {
    task: "resolveKubernetes",
    references: ["pods/delete"],
    declaredPermissions: null,
    tier: "read-only",
  },
]

test("committed browser compiler matches native Rust success and rejection behavior", () => {
  requests.push(
    ...["diagnostics", "remediation"].map(mode => ({
      task: "compileKubernetes",
      mode,
      grants: [
        {
          apiGroup: "",
          resource: "pods",
          verbs: ["get", "delete"],
          resourceNames: ["selected"],
          sources: [],
        },
      ],
    })),
  )
  const output = execFileSync(
    "cargo",
    [
      "run",
      "--quiet",
      "--manifest-path",
      resolve(root, "Cargo.toml"),
      "-p",
      "alien-permissions",
      "--no-default-features",
      "--bin",
      "operation-permission-compiler",
    ],
    {
      cwd: root,
      input: `${requests.map(request => JSON.stringify(request)).join("\n")}\n`,
      encoding: "utf8",
    },
  )
    .trim()
    .split("\n")
    .map(line => JSON.parse(line))
  const browser = requests.map(request =>
    JSON.parse(executeOperationPermissions(JSON.stringify(request))),
  )
  assert.deepEqual(browser, output)
  assert.deepEqual(
    browser.map(result => result.ok),
    [true, true, false, true, false, true, true, false, true, true],
  )
})

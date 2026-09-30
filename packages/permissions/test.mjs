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
  { task: "kubernetesOperatorRuntime" },
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
  const invalidGrants = [
    {
      task: "compileAws",
      statements: [
        {
          ...statements[0],
          actions: ["iam:PassRole"],
          resources: ["arn:aws:iam::123456789012:role/administrator"],
        },
      ],
      ceilings: { s3BucketArns: [], sqsQueueArns: [] },
    },
    ...[[], ["example-bucket"]].flatMap(gcsBucketNames =>
      ["iam.serviceAccounts.getAccessToken", "storage.objects.list"].map(permission => ({
        task: "compileGcp",
        grants: [{ permission, scope: `projects/\${projectName}`, sources: [] }],
        ceilings: { gcsBucketNames },
      })),
    ),
    ...["diagnostics", "remediation"].flatMap(mode =>
      [
        { resource: "secrets", verbs: ["get"], resourceNames: [] },
        { resource: "pods/exec", verbs: ["create"], resourceNames: [] },
        { resource: "pods", verbs: ["*"], resourceNames: [] },
        { resource: "pods", verbs: ["delete"], resourceNames: ["*"] },
      ].map(grant => ({
        task: "compileKubernetes",
        mode,
        grants: [{ apiGroup: "", ...grant, sources: [] }],
      })),
    ),
  ]
  requests.push(...invalidGrants)
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
    [
      true,
      true,
      true,
      false,
      true,
      false,
      true,
      true,
      false,
      true,
      true,
      ...invalidGrants.map(() => false),
    ],
  )
})

test("browser runtime task returns the six required inventory grants", () => {
  const result = JSON.parse(executeOperationPermissions('{"task":"kubernetesOperatorRuntime"}'))
  assert.equal(result.ok, true)
  assert.deepEqual(
    result.value.map(({ apiGroup, resource, verbs, resourceNames }) => ({
      apiGroup,
      resource,
      verbs,
      resourceNames,
    })),
    [
      ["apps", "deployments"],
      ["apps", "statefulsets"],
      ["apps", "daemonsets"],
      ["", "pods"],
      ["", "events"],
      ["metrics.k8s.io", "pods"],
    ].map(([apiGroup, resource]) => ({ apiGroup, resource, verbs: ["list"], resourceNames: [] })),
  )
  assert.ok(result.value.every(rule => typeof rule.reason === "string" && rule.reason.length > 0))
})

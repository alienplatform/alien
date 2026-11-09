import * as alien from "@alienplatform/core"

const releaseRead: alien.PermissionSet = {
  id: "acme/release-read",
  description: "Read release artifacts across the stack.",
  platforms: {
    aws: [
      {
        label: "Read artifacts",
        description: "Allows reading release objects.",
        effect: "Allow",
        grant: { actions: ["s3:GetObject", "s3:ListBucket"] },
        binding: {
          stack: {
            resources: ["arn:aws:s3:::${stackPrefix}-*"],
            notResources: ["arn:aws:s3:::${stackPrefix}-scratch/*"],
            condition: {
              StringEquals: {
                "aws:ResourceTag/team": "acme",
                "aws:RequestedRegion": ["us-east-1", "eu-west-1"],
              },
            },
          },
        },
      },
      {
        effect: "Deny",
        grant: { actions: ["s3:DeleteObject"] },
        binding: {
          resource: {
            resources: ["arn:aws:s3:::${stackPrefix}-archive/*"],
            notResources: ["arn:aws:s3:::${stackPrefix}-archive/keep/*"],
            condition: { Bool: { "aws:SecureTransport": "false" } },
          },
        },
      },
    ],
    gcp: [
      {
        label: "Read buckets",
        description: "Allows reading release buckets.",
        grant: {
          permissions: ["storage.objects.get"],
          predefinedRoles: ["roles/storage.objectViewer"],
        },
        binding: {
          stack: {
            scope: "projects/${projectId}",
            condition: {
              title: "release only",
              expression: "resource.name.startsWith('projects/_/buckets/acme-')",
            },
          },
        },
      },
      {
        grant: { permissions: ["storage.objects.list"] },
        binding: {
          resource: {
            scope: "projects/${projectId}/buckets/${resourceName}",
            condition: {
              title: "named bucket",
              expression: "resource.name.endsWith('-archive')",
            },
          },
        },
      },
    ],
    azure: [
      {
        label: "Read blobs",
        description: "Allows reading release blobs.",
        grant: {
          actions: ["Microsoft.Storage/storageAccounts/read"],
          dataActions: [
            "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read",
          ],
          residualPermissions: ["Microsoft.Storage/storageAccounts/listKeys/action"],
        },
        binding: {
          stack: {
            scope: "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}",
          },
        },
      },
      {
        grant: {
          dataActions: [
            "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/write",
          ],
        },
        binding: {
          resource: {
            scope:
              "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}/providers/Microsoft.Storage/storageAccounts/${resourceName}",
          },
        },
      },
    ],
  },
}

const empty: alien.PermissionSet = {
  id: "acme/empty",
  description: "A set no platform grants anything for.",
  platforms: {},
}

const deployer = new alien.ServiceAccount("deployer")
  .displayName("Deployer identity")
  .description("Identity the release pipeline assumes.")
  .stackPermissionSet(releaseRead)
  .stackPermissionSet(empty)
  .build()

// The builder has no method for resource-scoped permission sets, so the generated config
// field is set directly.
;(deployer.config as { resourcePermissionSets?: Record<string, alien.PermissionSet[]> }).resourcePermissionSets = {
  archive: [
    {
      id: "acme/archive-read",
      description: "Read the archive bucket.",
      platforms: {
        aws: [
          {
            grant: { actions: ["s3:GetObject"] },
            binding: { resource: { resources: ["arn:aws:s3:::${stackPrefix}-archive/*"] } },
          },
        ],
      },
    },
  ],
}

const plain = new alien.ServiceAccount("plain").build()

export default new alien.Stack("acme-identity")
  .platforms(["aws", "gcp", "azure"])
  .add(deployer, "live", { remoteAccess: true })
  .add(plain, "frozen")
  .build()

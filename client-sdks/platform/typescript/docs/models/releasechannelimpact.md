# ReleaseChannelImpact

## Example Usage

```typescript
import { ReleaseChannelImpact } from "@alienplatform/platform-api/models";

let value: ReleaseChannelImpact = {
  channel: "<value>",
  currentRelease: {
    id: "rel_WbhQgksrawSKIpEN0NAssHX9",
    version: "<value>",
    gitMetadata: {
      commitSha: "dc36199b2234c6586ebe05ec94078a895c707e29",
      commitMessage:
        "add method to measure Interaction to Next Paint (INP) (#36490)",
      commitRef: "main",
      commitDate: new Date("2026-03-16T12:00:00Z"),
      dirty: true,
      remoteUrl: "https://github.com/alienplatform/alien",
      commitAuthorName: "John Doe",
      commitAuthorEmail: "john@example.com",
      commitAuthorLogin: "johndoe",
      commitAuthorAvatarUrl: "https://github.com/johndoe.png",
    },
    createdAt: new Date("2024-11-28T16:08:04.228Z"),
  },
  runsThisRelease: false,
  rollbackRelease: {
    id: "rel_WbhQgksrawSKIpEN0NAssHX9",
    version: "<value>",
    gitMetadata: {
      commitSha: "dc36199b2234c6586ebe05ec94078a895c707e29",
      commitMessage:
        "add method to measure Interaction to Next Paint (INP) (#36490)",
      commitRef: "main",
      commitDate: new Date("2026-03-16T12:00:00Z"),
      dirty: true,
      remoteUrl: "https://github.com/alienplatform/alien",
      commitAuthorName: "John Doe",
      commitAuthorEmail: "john@example.com",
      commitAuthorLogin: "johndoe",
      commitAuthorAvatarUrl: "https://github.com/johndoe.png",
    },
    createdAt: new Date("2024-07-08T16:46:13.446Z"),
  },
  deploymentCount: 441952,
  rolloutEffects: {
    updates: 671036,
    unconfirmed: 236980,
    retries: 867526,
    alreadyOn: 936624,
    pinned: 689029,
    customerApplies: 432608,
    waitsForApproval: 497865,
    waitsForOwner: 982956,
    waitsForInputs: 81853,
    notAffected: 249010,
  },
  effects: {
    updates: 800610,
    unconfirmed: 261974,
    retries: 763897,
    alreadyOn: 803341,
    pinned: 341754,
    customerApplies: 303262,
    waitsForApproval: 381269,
    waitsForOwner: 794437,
    waitsForInputs: 898323,
    notAffected: 33117,
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `channel`                                                          | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `currentRelease`                                                   | [models.DeploymentReleaseInfo](../models/deploymentreleaseinfo.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `runsThisRelease`                                                  | *boolean*                                                          | :heavy_check_mark:                                                 | Whether the channel already points to this release                 |
| `rollbackRelease`                                                  | [models.DeploymentReleaseInfo](../models/deploymentreleaseinfo.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `deploymentCount`                                                  | *number*                                                           | :heavy_check_mark:                                                 | Deployments that follow the channel                                |
| `rolloutEffects`                                                   | [models.PromotionEffectCounts](../models/promotioneffectcounts.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `effects`                                                          | [models.PromotionEffectCounts](../models/promotioneffectcounts.md) | :heavy_check_mark:                                                 | N/A                                                                |
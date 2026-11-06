# Release

## Example Usage

```typescript
import { Release } from "@alienplatform/platform-api/models";

let value: Release = {
  id: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
  projectId: "<id>",
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
  createdAt: new Date("2026-08-08T13:05:28.803Z"),
  setupFingerprints: {},
  workspaceId: "<id>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the release.                                                            | rel_WbhQgksrawSKIpEN0NAssHX9E2Su                                                              |
| `projectId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `version`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `title`                                                                                       | *string*                                                                                      | :heavy_minus_sign:                                                                            | Human-readable release title                                                                  |                                                                                               |
| `gitMetadata`                                                                                 | [models.GitMetadata](../models/gitmetadata.md)                                                | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `stack`                                                                                       | [models.ReleaseStack](../models/releasestack.md)                                              | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `signedStacks`                                                                                | [models.ReleaseSignedStacks](../models/releasesignedstacks.md)                                | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `signing`                                                                                     | [models.ReleaseSigning](../models/releasesigning.md)                                          | :heavy_minus_sign:                                                                            | Who signed the release and for which channel; null when unsigned                              |                                                                                               |
| `setupFingerprints`                                                                           | Record<string, [models.SetupFingerprintInfo](../models/setupfingerprintinfo.md)>              | :heavy_check_mark:                                                                            | Per-target setup compatibility fingerprints for this release                                  |                                                                                               |
| `rootDirectory`                                                                               | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `createdByUserId`                                                                             | *string*                                                                                      | :heavy_minus_sign:                                                                            | ID of the platform user who created the release, if known                                     |                                                                                               |
| `workspaceId`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
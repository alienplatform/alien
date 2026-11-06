# CurrentReleaseFile

A file delivered from the install-owned input store after a signed release
authorizes its exact destination. Values are never part of the release.

## Example Usage

```typescript
import { CurrentReleaseFile } from "@alienplatform/platform-api/models";

let value: CurrentReleaseFile = {
  path: "/usr/src",
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `envName`                                                      | *string*                                                       | :heavy_minus_sign:                                             | Optional environment variable containing the destination path. |
| `path`                                                         | *string*                                                       | :heavy_check_mark:                                             | Absolute destination inside the workload.                      |
| `targetResources`                                              | *string*[]                                                     | :heavy_minus_sign:                                             | Resource IDs or prefix patterns; absent selects all workloads. |
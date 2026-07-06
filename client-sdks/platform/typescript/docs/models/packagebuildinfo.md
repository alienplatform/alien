# PackageBuildInfo

Source provenance for a generated CLI package.

## Example Usage

```typescript
import { PackageBuildInfo } from "@alienplatform/platform-api/models";

let value: PackageBuildInfo = {
  alienSha: "<value>",
  horizonSha: "<value>",
  platformSha: "<value>",
  sourceCliBinarySha256: "<value>",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `alienSha`                                                                                       | *string*                                                                                         | :heavy_check_mark:                                                                               | Alien source commit used to build the source CLI and agent binaries.                             |
| `horizonSha`                                                                                     | *string*                                                                                         | :heavy_check_mark:                                                                               | Horizon source commit used by platform private extensions, if applicable.                        |
| `platformSha`                                                                                    | *string*                                                                                         | :heavy_check_mark:                                                                               | Platform source commit used to build packages-builder and private extensions.                    |
| `sourceAgentBinarySha256`                                                                        | *string*                                                                                         | :heavy_minus_sign:                                                                               | SHA256 checksum of the source companion agent binary shipped with the CLI package, when present. |
| `sourceCliBinarySha256`                                                                          | *string*                                                                                         | :heavy_check_mark:                                                                               | SHA256 checksum of the source deploy CLI binary before white-label config is appended.           |
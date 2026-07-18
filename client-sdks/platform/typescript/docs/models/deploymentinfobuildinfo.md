# DeploymentInfoBuildInfo

Source provenance for a generated CLI package.

## Example Usage

```typescript
import { DeploymentInfoBuildInfo } from "@alienplatform/platform-api/models";

let value: DeploymentInfoBuildInfo = {
  alienSha: "<value>",
  horizonSha: "<value>",
  platformSha: "<value>",
  sourceCliBinarySha256: "<value>",
};
```

## Fields

| Field                                                                                           | Type                                                                                            | Required                                                                                        | Description                                                                                     |
| ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `alienSha`                                                                                      | *string*                                                                                        | :heavy_check_mark:                                                                              | Alien source commit used to build the source CLI and agent binaries.                            |
| `horizonSha`                                                                                    | *string*                                                                                        | :heavy_check_mark:                                                                              | Compute backend source revision used by optional package extensions, if applicable.             |
| `machineBundleManifestUrl`                                                                      | *string*                                                                                        | :heavy_minus_sign:                                                                              | Machine runtime release manifest embedded into the generated CLI.                               |
| `platformSha`                                                                                   | *string*                                                                                        | :heavy_check_mark:                                                                              | Source revision used to build the package service and optional extensions.                      |
| `sourceAgentBinarySha256`                                                                       | *string*                                                                                        | :heavy_minus_sign:                                                                              | SHA256 checksum of the source runtime helper binary shipped with the CLI package, when present. |
| `sourceCliBinarySha256`                                                                         | *string*                                                                                        | :heavy_check_mark:                                                                              | SHA256 checksum of the source deploy CLI binary before white-label config is appended.          |
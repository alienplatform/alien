# ConfigSandboxBundle

Configuration for a sandbox bundle package.

The bundle is a zip holding one Dockerfile that layers the published Alien sandbox agent
onto the vendor's base image; everything the builder writes is derived from these fields.

## Example Usage

```typescript
import { ConfigSandboxBundle } from "@alienplatform/platform-api/models";

let value: ConfigSandboxBundle = {
  agentImage: "<value>",
  baseImage: "<value>",
  objectKey: "<value>",
  type: "sandbox-bundle",
};
```

## Fields

| Field                                                                                                                                                                   | Type                                                                                                                                                                    | Required                                                                                                                                                                | Description                                                                                                                                                             |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `agentImage`                                                                                                                                                            | *string*                                                                                                                                                                | :heavy_check_mark:                                                                                                                                                      | Full reference of the published sandbox agent image copied into the bundle.                                                                                             |
| `baseImage`                                                                                                                                                             | *string*                                                                                                                                                                | :heavy_check_mark:                                                                                                                                                      | The base container image the sandbox filesystem starts from. A release-pushed image<br/>arrives resolved onto the private registry template with its {region} token intact. |
| `objectKey`                                                                                                                                                             | *string*                                                                                                                                                                | :heavy_check_mark:                                                                                                                                                      | Object key the bundle is written under in every regional bundle store.                                                                                                  |
| `supportedAwsRegions`                                                                                                                                                   | *string*[]                                                                                                                                                              | :heavy_minus_sign:                                                                                                                                                      | Regions whose bundle store must receive this bundle. Part of the build input hash,<br/>so a newly supported region re-mints the bundle instead of silently missing it.  |
| `type`                                                                                                                                                                  | *"sandbox-bundle"*                                                                                                                                                      | :heavy_check_mark:                                                                                                                                                      | N/A                                                                                                                                                                     |
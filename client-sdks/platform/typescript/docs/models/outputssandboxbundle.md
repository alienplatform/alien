# OutputsSandboxBundle

Outputs from a sandbox bundle package build.

## Example Usage

```typescript
import { OutputsSandboxBundle } from "@alienplatform/platform-api/models";

let value: OutputsSandboxBundle = {
  bundleUriTemplate: "<value>",
  objectKey: "<value>",
  regions: [],
  sha256: "<value>",
  size: 830757,
  type: "sandbox-bundle",
};
```

## Fields

| Field                                                                                                                                                  | Type                                                                                                                                                   | Required                                                                                                                                               | Description                                                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `bundleUriTemplate`                                                                                                                                    | *string*                                                                                                                                               | :heavy_check_mark:                                                                                                                                     | Bundle URI with the {region} token in the bucket, resolved by the deploy-time emitters.                                                                |
| `objectKey`                                                                                                                                            | *string*                                                                                                                                               | :heavy_check_mark:                                                                                                                                     | Object key the bundle was written under in every regional bundle store.                                                                                |
| `regions`                                                                                                                                              | *string*[]                                                                                                                                             | :heavy_check_mark:                                                                                                                                     | Regions whose bundle store received this bundle.                                                                                                       |
| `sha256`                                                                                                                                               | *string*                                                                                                                                               | :heavy_check_mark:                                                                                                                                     | SHA256 checksum of the region-neutral bundle zip; a region-templated base image makes<br/>each regional object differ from it only in the rendered region. |
| `size`                                                                                                                                                 | *number*                                                                                                                                               | :heavy_check_mark:                                                                                                                                     | Region-neutral bundle zip size in bytes.                                                                                                               |
| `type`                                                                                                                                                 | [models.OutputsTypeSandboxBundle](../models/outputstypesandboxbundle.md)                                                                               | :heavy_check_mark:                                                                                                                                     | N/A                                                                                                                                                    |
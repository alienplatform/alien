# DeploymentDetailResponseGcpSandboxScaffolding

A Frozen GCP sandbox's image repository and the manager's write access to it. Setup makes
them because the runtime identity may write images into the repository but cannot create
it or change who may; an image-only release then rolls without setup.

Kept in its own `RuntimeMetadata` field, not as a [`SetupScaffolding`] variant: a manager or
CLI from before this record cannot read an unknown variant of that tagged enum, and would then
fail to load the whole deployment state. An unknown field is ignored instead.

## Example Usage

```typescript
import { DeploymentDetailResponseGcpSandboxScaffolding } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseGcpSandboxScaffolding = {
  imageRepository: "<value>",
  writeRole: "<value>",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `imageRepository`                                                  | *string*                                                           | :heavy_check_mark:                                                 | Artifact Registry repository the sandbox's image is copied into.   |
| `writeRole`                                                        | *string*                                                           | :heavy_check_mark:                                                 | Custom role (no delete) the manager is granted on that repository. |
| `writerGranted`                                                    | *boolean*                                                          | :heavy_minus_sign:                                                 | Whether the manager holds `write_role` on the repository.          |
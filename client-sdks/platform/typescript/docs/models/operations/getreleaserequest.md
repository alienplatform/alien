# GetReleaseRequest

## Example Usage

```typescript
import { GetReleaseRequest } from "@alienplatform/platform-api/models/operations";

let value: GetReleaseRequest = {
  id: "rel_WbhQgksrawSKIpEN0NAssHX9",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    | Example                                                                        |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `id`                                                                           | *string*                                                                       | :heavy_check_mark:                                                             | Unique identifier for the release.                                             | rel_WbhQgksrawSKIpEN0NAssHX9                                                   |
| `include`                                                                      | [operations.GetReleaseInclude](../../models/operations/getreleaseinclude.md)[] | :heavy_minus_sign:                                                             | Optional fields to include: project, rollout                                   |                                                                                |
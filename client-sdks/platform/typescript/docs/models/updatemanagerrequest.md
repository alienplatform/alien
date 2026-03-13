# UpdateManagerRequest

Request schema for updating a manager to a new release

## Example Usage

```typescript
import { UpdateManagerRequest } from "@alienplatform/platform-api/models";

let value: UpdateManagerRequest = {
  releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          | Example                                                                              |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `releaseId`                                                                          | *string*                                                                             | :heavy_minus_sign:                                                                   | Optional release ID to update to. If not provided, the active release will be chosen | rel_WbhQgksrawSKIpEN0NAssHX9                                                         |
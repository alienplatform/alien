# Workspace

## Example Usage

```typescript
import { Workspace } from "@aliendotdev/platform-api/models";

let value: Workspace = {
  id: "ws_It13CUaGEhLLAB87simX0",
  name: "my-workspace",
  createdAt: new Date("2025-09-06T14:20:55.891Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the workspace.                                                          | ws_It13CUaGEhLLAB87simX0                                                                      |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | Workspace name.                                                                               | my-workspace                                                                                  |
| `logoUrl`                                                                                     | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
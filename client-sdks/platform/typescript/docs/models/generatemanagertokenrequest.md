# GenerateManagerTokenRequest

## Example Usage

```typescript
import { GenerateManagerTokenRequest } from "@alienplatform/platform-api/models";

let value: GenerateManagerTokenRequest = {};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `project`                                                                                                                      | *string*                                                                                                                       | :heavy_minus_sign:                                                                                                             | Project ID or name to scope token access to. When omitted, the token is scoped to all projects accessible by the current user. |
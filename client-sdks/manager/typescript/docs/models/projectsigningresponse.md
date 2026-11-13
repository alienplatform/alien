# ProjectSigningResponse

What a release of the project needs to know to sign its setup templates.

## Example Usage

```typescript
import { ProjectSigningResponse } from "@alienplatform/manager-api/models";

let value: ProjectSigningResponse = {
  setupHosting: {},
};
```

## Fields

| Field                                                                                                             | Type                                                                                                              | Required                                                                                                          | Description                                                                                                       |
| ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `setupHosting`                                                                                                    | [models.SetupHosting](../models/setuphosting.md)                                                                  | :heavy_check_mark:                                                                                                | What this manager renders setup templates with, so the digests a<br/>release signs are of the templates installs get. |

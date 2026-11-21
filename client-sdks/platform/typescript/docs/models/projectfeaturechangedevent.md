# ProjectFeatureChangedEvent

## Example Usage

```typescript
import { ProjectFeatureChangedEvent } from "@alienplatform/platform-api/models";

let value: ProjectFeatureChangedEvent = {
  type: "ProjectFeatureChanged",
  feature: "remoteSandbox",
  enabled: true,
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `type`                                                                                 | *"ProjectFeatureChanged"*                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `feature`                                                                              | [models.Feature](../models/feature.md)                                                 | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `enabled`                                                                              | *boolean*                                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `actor`                                                                                | [models.ProjectFeatureChangedEventActor](../models/projectfeaturechangedeventactor.md) | :heavy_minus_sign:                                                                     | Authenticated principal that requested a deployment intent event.                      |
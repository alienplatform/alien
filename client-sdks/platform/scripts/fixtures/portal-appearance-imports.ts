import type {
  UpdateProjectDeploymentPortalAppearance,
  UpdateProjectDeploymentPortalAppearance$Outbound,
} from "../../typescript/esm/models/index.js";
import {
  UpdateProjectDeploymentPortalAppearance$outboundSchema,
  updateProjectDeploymentPortalAppearanceToJSON,
} from "../../typescript/esm/models/index.js";
import type {
  UpdateProjectDeploymentPortalAppearance as DeepAppearance,
  UpdateProjectDeploymentPortalAppearance$Outbound as DeepOutbound,
} from "../../typescript/esm/models/updateproject.js";
import {
  UpdateProjectDeploymentPortalAppearance$outboundSchema as deepSchema,
  updateProjectDeploymentPortalAppearanceToJSON as deepToJSON,
} from "../../typescript/esm/models/updateproject.js";

const appearance: UpdateProjectDeploymentPortalAppearance = { title: "Acme" };
const deepAppearance: DeepAppearance = appearance;
const outbound: UpdateProjectDeploymentPortalAppearance$Outbound =
  UpdateProjectDeploymentPortalAppearance$outboundSchema.parse(appearance);
const deepOutbound: DeepOutbound = deepSchema.parse(deepAppearance);
updateProjectDeploymentPortalAppearanceToJSON(appearance);
deepToJSON(deepAppearance);
void outbound;
void deepOutbound;

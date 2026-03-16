terraform {
  required_providers {
    test-app = {
      source  = "b949d1f86353.ngrok.app/alienplatform/test-app"
      version = "~> 1.0"
    }
  }
}

provider "test-app" {
  # TODO: Replace with your actual agent key
  agent_key = "ax_agent_WtfG0Y7FLIrgHK58A13UmBlVoobsyIYBFGeUgvk6"
  
  # Local dev platform
  base_url = "http://localhost:8080"
}

resource "test-app_agent" "test" {
  name     = "terraform-dev-test"
  platform = "aws"
  
  # Terraform handles all deployments
  no_auto_updates = true
}

output "agent_id" {
  value = test-app_agent.test.agent_id
}

output "agent_status" {
  value = test-app_agent.test.status
}


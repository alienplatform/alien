# alien-operator

Pull-model agent that manages deployments in remote environments. 

Syncs with the alien-manager on a regular interval, runs `alien_deployment::step()` when updates are available, and forwards telemetry.

Main entry: `run_operator(config, service_provider)` — starts the sync loop with cancellation support.

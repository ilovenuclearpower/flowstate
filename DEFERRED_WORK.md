# Deferred Work

## Dynamic RunPod Instance Management

- **Feature:** The `flowstate-server` should be able to dynamically manage RunPod instances for tasks requiring the "standard" capability.
- **Trigger:** When a task requiring a "standard" capability runner is available, and no such runner has polled for work recently.
- **Action (Spin-Up):** The server should use the RunPod API to start a pre-configured GPU pod.
- **Action (Spin-Down):** The server should monitor the instance and automatically terminate it after a configurable period of inactivity to minimize costs.
- **Goal:** Provide on-demand, cost-effective compute for specific task types without requiring a persistent, manually-managed runner.

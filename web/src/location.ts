import { advanceLocationSimulation, locationSimulationFromRoute } from "@stadiamaps/ferrostar";

export class SimulatedLocationProvider {
  private simulationState = null;

  lastLocation = null;
  lastHeading = null;
  warpFactor = 1;

  updateCallback: () => void = () => {};

  setSimulatedRoute(route: any) {
    this.simulationState = locationSimulationFromRoute(route, 10.0);
    this.start();
  }

  async start() {
    while (this.simulationState !== null) {
      await new Promise((resolve) => setTimeout(resolve, (1 / this.warpFactor) * 1000));
      const initialState = this.simulationState;
      const updatedState = advanceLocationSimulation(initialState);

      if (initialState === updatedState) {
        return;
      }

      this.simulationState = updatedState;
      this.lastLocation = updatedState.current_location;

      // Since Lit cannot detect changes inside objects, here we use a callback to trigger a re-render
      // This is a minimal approach if we don't want to use a state management library like MobX, but might not be the ideal solution
      if (this.updateCallback) {
        this.updateCallback();
      }
    }
  }

  stop() {
    this.simulationState = null;
  }
}

export class BrowserLocationProvider {
  private geolocationWatchId: number | null = null;
  lastLocation: any = null;
  lastHeading = null;

  updateCallback: () => void = () => {};

  start() {
    if (navigator.geolocation && !this.geolocationWatchId) {
      const options = {
        enableHighAccuracy: true,
      };

      this.geolocationWatchId = navigator.geolocation.watchPosition((position: GeolocationPosition) => {
        let speed = null;
        if (position.coords.speed) {
          speed = {
            value: position.coords.speed
          }
        }
        this.lastLocation = {
            coordinates: { lat: position.coords.latitude, lng: position.coords.longitude },
            horizontalAccuracy: position.coords.accuracy,
            courseOverGround: {
              degrees: Math.floor(position.coords.heading || 0),
            },
            timestamp: position.timestamp,
            speed: speed,
          };

          if (this.updateCallback) {
            this.updateCallback();
          }
        },
        // TODO: Better alert mechanism
        (error) => alert(error.message),
        options
      );
    }
  }

  stop() {
    this.lastLocation = null;
    if (navigator.geolocation && this.geolocationWatchId) {
      navigator.geolocation.clearWatch(this.geolocationWatchId);
      this.geolocationWatchId = null;
    }
  }
}

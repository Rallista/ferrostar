//! High-level HTTP request generation for Valhalla HTTP APIs.

use super::{RouteRequest, RoutingRequestGenerationError};
use crate::models::{UserLocation, Waypoint, WaypointKind};
use crate::routing_adapters::RouteRequestGenerator;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::collections::BTreeMap as HashMap;
use serde_json::{json, Value as JsonValue};
#[cfg(feature = "std")]
use std::collections::HashMap;

#[cfg(feature = "alloc")]
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

/// A route request generator for Valhalla backends operating over HTTP.
///
/// Valhalla supports the [`WaypointKind`] field of [`Waypoint`]s. Variants have the same meaning as their
/// [`type` strings in Valhalla API](https://valhalla.github.io/valhalla/api/turn-by-turn/api-reference/#locations)
/// having the same name.
#[derive(Debug)]
pub struct ValhallaHttpRequestGenerator {
    /// The full URL of the Valhalla endpoint to access. This will normally be the route endpoint,
    /// but the optimized route endpoint should be interchangeable.
    ///
    /// Users *may* include a query string with an API key.
    endpoint_url: String,
    /// The Valhalla costing model to use.
    profile: String,
    // TODO: Language, units, and other top-level parameters
    /// JSON costing options to pass through.
    costing_options: JsonValue,
}

impl ValhallaHttpRequestGenerator {
    pub fn new(endpoint_url: String, profile: String, costing_options: Option<JsonValue>) -> Self {
        Self {
            endpoint_url,
            profile,
            costing_options: costing_options.unwrap_or(json!({})),
        }
    }

    pub fn with_costing_options_json(
        endpoint_url: String,
        profile: String,
        costing_options_json: Option<String>,
    ) -> Result<Self, serde_json::Error> {
        let parsed_costing_options: JsonValue = match costing_options_json.as_deref() {
            Some(options) => serde_json::from_str(options)?,
            None => json!({}),
        };
        Ok(Self {
            endpoint_url,
            profile,
            costing_options: parsed_costing_options,
        })
    }
}

impl RouteRequestGenerator for ValhallaHttpRequestGenerator {
    fn generate_request(
        &self,
        user_location: UserLocation,
        waypoints: Vec<Waypoint>,
    ) -> Result<RouteRequest, RoutingRequestGenerationError> {
        if waypoints.is_empty() {
            Err(RoutingRequestGenerationError::NotEnoughWaypoints)
        } else {
            let headers =
                HashMap::from([("Content-Type".to_string(), "application/json".to_string())]);
            let mut start = json!({
                "lat": user_location.coordinates.lat,
                "lon": user_location.coordinates.lng,
                // TODO: Street side tolerance as a tunable
                "street_side_tolerance": core::cmp::max(5, user_location.horizontal_accuracy as u16),
            });
            // TODO: Tunable to decide whether we care about course, and how accurate it needs to be
            if let Some(course) = user_location.course_over_ground {
                start["heading"] = course.degrees.into();
            }

            let locations: Vec<JsonValue> = core::iter::once(start)
                .chain(waypoints.iter().map(|waypoint| {
                    json!({
                        "lat": waypoint.coordinate.lat,
                        "lon": waypoint.coordinate.lng,
                        "type": match waypoint.kind {
                            WaypointKind::Break => "break",
                            WaypointKind::Via => "via",
                        },
                    })
                }))
                .collect();

            // NOTE: We currently use the OSRM format, as it is the richest one.
            // Though it would be nice to use PBF if we can get the required data.
            // However, certain info (like banners) are only available in the OSRM format.
            // TODO: Trace attributes as we go rather than pulling a fat payload upfront that we might ditch later?
            let args = json!({
                "format": "osrm",
                "filters": {
                    "action": "include",
                    "attributes": [
                      "shape_attributes.speed",
                      "shape_attributes.speed_limit",
                      "shape_attributes.time",
                      "shape_attributes.length"
                    ]
                },
                "banner_instructions": true,
                "voice_instructions": true,
                "costing": &self.profile,
                "locations": locations,
                "costing_options": &self.costing_options,
            });
            let body = serde_json::to_vec(&args)?;
            Ok(RouteRequest::HttpPost {
                url: self.endpoint_url.clone(),
                headers,
                body,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CourseOverGround, GeographicCoordinate};
    use assert_json_diff::assert_json_include;
    use serde_json::{from_slice, json};

    #[cfg(all(feature = "std", not(feature = "web-time")))]
    use std::time::SystemTime;

    #[cfg(feature = "web-time")]
    use web_time::SystemTime;
    const ENDPOINT_URL: &str = "https://api.stadiamaps.com/route/v1";
    const COSTING: &str = "bicycle";
    const USER_LOCATION: UserLocation = UserLocation {
        coordinates: GeographicCoordinate { lat: 0.0, lng: 0.0 },
        horizontal_accuracy: 6.0,
        course_over_ground: None,
        timestamp: SystemTime::UNIX_EPOCH,
        speed: None,
    };
    const USER_LOCATION_WITH_COURSE: UserLocation = UserLocation {
        coordinates: GeographicCoordinate { lat: 0.0, lng: 0.0 },
        horizontal_accuracy: 6.0,
        course_over_ground: Some(CourseOverGround {
            degrees: 42,
            accuracy: Some(12),
        }),
        timestamp: SystemTime::UNIX_EPOCH,
        speed: None,
    };
    const WAYPOINTS: [Waypoint; 2] = [
        Waypoint {
            coordinate: GeographicCoordinate { lat: 0.0, lng: 1.0 },
            kind: WaypointKind::Break,
        },
        Waypoint {
            coordinate: GeographicCoordinate { lat: 2.0, lng: 3.0 },
            kind: WaypointKind::Break,
        },
    ];

    #[test]
    fn not_enough_locations() {
        let generator =
            ValhallaHttpRequestGenerator::new(ENDPOINT_URL.to_string(), COSTING.to_string(), None);

        // At least two locations are required
        assert!(matches!(
            generator.generate_request(USER_LOCATION, Vec::new()),
            Err(RoutingRequestGenerationError::NotEnoughWaypoints)
        ));
    }

    fn generate_body(
        user_location: UserLocation,
        waypoints: Vec<Waypoint>,
        costing_options_json: Option<String>,
    ) -> JsonValue {
        let generator = ValhallaHttpRequestGenerator::with_costing_options_json(
            ENDPOINT_URL.to_string(),
            COSTING.to_string(),
            costing_options_json,
        )
        .expect("Unable to create request generator");

        match generator.generate_request(user_location, waypoints) {
            Ok(RouteRequest::HttpPost {
                url: request_url,
                headers,
                body,
            }) => {
                assert_eq!(ENDPOINT_URL, request_url);
                assert_eq!(headers["Content-Type"], "application/json".to_string());
                from_slice(&body).expect("Failed to parse request body as JSON")
            }
            Ok(RouteRequest::HttpGet { .. }) => unreachable!(
                "The Valhalla HTTP request generator currently only generates POST requests"
            ),
            Err(e) => {
                println!("Failed to generate request: {:?}", e);
                json!(null)
            }
        }
    }

    #[test]
    fn request_body_without_course() {
        let body_json = generate_body(USER_LOCATION, WAYPOINTS.to_vec(), None);

        assert_json_include!(
            actual: body_json,
            expected: json!({
                "costing": COSTING,
                "locations": [
                    {
                        "lat": 0.0,
                        "lon": 0.0,
                        "street_side_tolerance": 6,
                    },
                    {
                        "lat": 0.0,
                        "lon": 1.0
                    },
                    {
                        "lat": 2.0,
                        "lon": 3.0,
                    }
                ],
            })
        );
    }

    #[test]
    fn request_body_with_course() {
        let body_json = generate_body(USER_LOCATION_WITH_COURSE, WAYPOINTS.to_vec(), None);

        assert_json_include!(
            actual: body_json,
            expected: json!({
                "costing": COSTING,
                "locations": [
                    {
                        "lat": 0.0,
                        "lon": 0.0,
                        "street_side_tolerance": 6,
                        "heading": 42,
                    },
                    {
                        "lat": 0.0,
                        "lon": 1.0
                    },
                    {
                        "lat": 2.0,
                        "lon": 3.0,
                    }
                ],
            })
        );
    }

    #[test]
    fn request_body_without_costing_options() {
        let body_json = generate_body(USER_LOCATION, WAYPOINTS.to_vec(), None);

        assert_json_include!(
            actual: body_json,
            expected: json!({
                "costing_options": {},
            })
        );
    }

    #[test]
    fn request_body_with_costing_options() {
        let body_json = generate_body(
            USER_LOCATION,
            WAYPOINTS.to_vec(),
            Some(r#"{"bicycle": {"bicycle_type": "Road"}}"#.to_string()),
        );

        assert_json_include!(
            actual: body_json,
            expected: json!({
                "costing_options": {
                    "bicycle": {
                        "bicycle_type": "Road",
                    },
                },
            })
        );
    }

    #[test]
    fn request_body_with_invalid_horizontal_accuracy() {
        let generator =
            ValhallaHttpRequestGenerator::new(ENDPOINT_URL.to_string(), COSTING.to_string(), None);
        let location = UserLocation {
            coordinates: GeographicCoordinate { lat: 0.0, lng: 0.0 },
            horizontal_accuracy: -6.0,
            course_over_ground: None,
            timestamp: SystemTime::now(),
            speed: None,
        };

        let RouteRequest::HttpPost {
            url: request_url,
            headers,
            body,
        } = generator
            .generate_request(location, WAYPOINTS.to_vec())
            .unwrap()
        else {
            unreachable!(
                "The Valhalla HTTP request generator currently only generates POST requests"
            );
        };

        assert_eq!(ENDPOINT_URL, request_url);
        assert_eq!(headers["Content-Type"], "application/json".to_string());

        let body_json: JsonValue = from_slice(&body).expect("Failed to parse request body as JSON");

        assert_json_include!(
            actual: body_json,
            expected: json!({
                "costing": COSTING,
                "locations": [
                    {
                        "lat": 0.0,
                        "lon": 0.0,
                        "street_side_tolerance": 5,
                    },
                    {
                        "lat": 0.0,
                        "lon": 1.0
                    },
                    {
                        "lat": 2.0,
                        "lon": 3.0,
                    }
                ],
            })
        );
    }
}

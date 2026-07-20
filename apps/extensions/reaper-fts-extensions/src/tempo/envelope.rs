//! Tempo Envelope wrapper
//!
//! Provides a wrapper around REAPER's tempo markers for manipulating tempo.
//! Uses the tempo marker API directly instead of raw envelope access.

use reaper_high::Reaper;
use tracing::debug;

/// Minimum BPM value allowed
pub const MIN_BPM: f64 = 1.0;

/// Maximum BPM value allowed
pub const MAX_BPM: f64 = 960.0;

/// Minimum time distance between tempo markers (in seconds)
pub const MIN_TEMPO_DIST: f64 = 0.0001;

/// Tempo point shape: Square (constant tempo until next point)
pub const SHAPE_SQUARE: i32 = 0;

/// Tempo point shape: Linear (ramp to next point)
pub const SHAPE_LINEAR: i32 = 1;

/// A tempo point in the tempo map
#[derive(Debug, Clone)]
pub struct TempoPoint {
    /// Time position in seconds
    pub time: f64,
    /// BPM value
    pub bpm: f64,
    /// Shape (0 = square, 1 = linear)
    pub shape: i32,
    /// Time signature numerator (if this is a time sig marker)
    pub sig_num: Option<i32>,
    /// Time signature denominator (if this is a time sig marker)
    pub sig_denom: Option<i32>,
}

/// Wrapper for REAPER's tempo markers
///
/// This provides a convenient interface for reading and modifying tempo markers.
pub struct TempoEnvelope {
    /// Cached points for efficient access
    points: Vec<TempoPoint>,
    /// Whether the envelope has been modified
    modified: bool,
}

impl TempoEnvelope {
    /// Create a new TempoEnvelope wrapper
    ///
    /// Loads all tempo markers from REAPER.
    pub fn new() -> Option<Self> {
        let mut wrapper = Self {
            points: Vec::new(),
            modified: false,
        };

        // Load all points
        wrapper.reload_points();

        if wrapper.points.is_empty() {
            return None;
        }

        Some(wrapper)
    }

    /// Reload points from REAPER's tempo markers
    fn reload_points(&mut self) {
        self.points.clear();

        let reaper = Reaper::get();
        let low = reaper.medium_reaper().low();

        let count = unsafe { low.CountTempoTimeSigMarkers(std::ptr::null_mut()) };

        for i in 0..count {
            let mut timepos = 0.0;
            let mut measurepos = 0;
            let mut beatpos = 0.0;
            let mut bpm = 0.0;
            let mut timesig_num = 0;
            let mut timesig_denom = 0;
            let mut lineartempo = false;

            let success = unsafe {
                low.GetTempoTimeSigMarker(
                    std::ptr::null_mut(),
                    i,
                    &mut timepos as *mut _,
                    &mut measurepos as *mut _,
                    &mut beatpos as *mut _,
                    &mut bpm as *mut _,
                    &mut timesig_num as *mut _,
                    &mut timesig_denom as *mut _,
                    &mut lineartempo as *mut _,
                )
            };

            if success {
                self.points.push(TempoPoint {
                    time: timepos,
                    bpm,
                    shape: if lineartempo {
                        SHAPE_LINEAR
                    } else {
                        SHAPE_SQUARE
                    },
                    sig_num: if timesig_num > 0 {
                        Some(timesig_num)
                    } else {
                        None
                    },
                    sig_denom: if timesig_denom > 0 {
                        Some(timesig_denom)
                    } else {
                        None
                    },
                });
            }
        }
    }

    /// Get the number of tempo points
    pub fn count_points(&self) -> usize {
        self.points.len()
    }

    /// Check if the envelope is locked
    pub fn is_locked(&self) -> bool {
        // Tempo envelope is typically not locked
        false
    }

    /// Get a tempo point by index
    pub fn get_point(&self, id: usize) -> Option<&TempoPoint> {
        self.points.get(id)
    }

    /// Set tempo point values
    ///
    /// Only non-None values will be updated.
    pub fn set_point(
        &mut self,
        id: usize,
        time: Option<f64>,
        bpm: Option<f64>,
        shape: Option<i32>,
    ) {
        if let Some(point) = self.points.get_mut(id) {
            if let Some(t) = time {
                point.time = t;
            }
            if let Some(b) = bpm {
                point.bpm = b;
            }
            if let Some(s) = shape {
                point.shape = s;
            }
            self.modified = true;
        }
    }

    /// Find a tempo point at the given time (within tolerance)
    ///
    /// Returns the index of the point, or None if not found.
    pub fn find(&self, time: f64, tolerance: f64) -> Option<usize> {
        for (i, point) in self.points.iter().enumerate() {
            if (point.time - time).abs() < tolerance {
                return Some(i);
            }
        }
        None
    }

    /// Find the closest tempo point to the given time
    ///
    /// Returns the index of the closest point.
    pub fn find_closest(&self, time: f64) -> Option<usize> {
        if self.points.is_empty() {
            return None;
        }

        let mut closest_id = 0;
        let mut closest_dist = f64::MAX;

        for (i, point) in self.points.iter().enumerate() {
            let dist = (point.time - time).abs();
            if dist < closest_dist {
                closest_dist = dist;
                closest_id = i;
            }
        }

        Some(closest_id)
    }

    /// Find the tempo point immediately before the given time
    ///
    /// Returns the index of the previous point.
    pub fn find_previous(&self, time: f64) -> Option<usize> {
        let mut prev_id = None;

        for (i, point) in self.points.iter().enumerate() {
            if point.time < time {
                prev_id = Some(i);
            } else {
                break;
            }
        }

        prev_id
    }

    /// Validate that a point ID exists
    pub fn validate_id(&self, id: usize) -> bool {
        id < self.points.len()
    }

    /// Get the BPM value at a position (interpolated)
    pub fn value_at_position(&self, time: f64) -> f64 {
        let reaper = Reaper::get();
        let low = reaper.medium_reaper().low();

        // Use REAPER's TimeMap2_GetDividedBpmAtTime for accurate BPM
        unsafe { low.TimeMap2_GetDividedBpmAtTime(std::ptr::null_mut(), time) }
    }

    /// Create a new tempo point
    ///
    /// Inserts a new point after the given index.
    pub fn create_point(&mut self, after_id: usize, time: f64, bpm: f64, shape: i32) -> bool {
        // Validate
        if !(MIN_BPM..=MAX_BPM).contains(&bpm) {
            return false;
        }

        let new_point = TempoPoint {
            time,
            bpm,
            shape,
            sig_num: None,
            sig_denom: None,
        };

        // Insert after the given ID
        let insert_pos = (after_id + 1).min(self.points.len());
        self.points.insert(insert_pos, new_point);
        self.modified = true;

        true
    }

    /// Commit changes to REAPER
    ///
    /// This writes all modified points back to REAPER's tempo markers.
    pub fn commit(&mut self) {
        if !self.modified {
            return;
        }

        let reaper = Reaper::get();
        let low = reaper.medium_reaper().low();

        // First, delete all existing tempo markers
        let count = unsafe { low.CountTempoTimeSigMarkers(std::ptr::null_mut()) };
        for i in (0..count).rev() {
            unsafe {
                low.DeleteTempoTimeSigMarker(std::ptr::null_mut(), i);
            }
        }

        // Insert all points
        for point in &self.points {
            unsafe {
                low.SetTempoTimeSigMarker(
                    std::ptr::null_mut(),         // project
                    -1,                           // ptidx (-1 to add new)
                    point.time,                   // timepos
                    -1,                           // measurepos (-1 to use timepos)
                    0.0,                          // beatpos
                    point.bpm,                    // bpm
                    point.sig_num.unwrap_or(0),   // timesig_num (0 for no change)
                    point.sig_denom.unwrap_or(0), // timesig_denom
                    point.shape == SHAPE_LINEAR,  // lineartempo
                );
            }
        }

        // Update timeline
        unsafe {
            low.UpdateTimeline();
        }

        self.modified = false;
        debug!("Committed {} tempo points", self.points.len());
    }
}

/// Move a tempo marker
///
/// This is the core algorithm from SWS BR_Tempo.cpp MoveTempo().
/// It moves a tempo marker and adjusts adjacent BPM values to maintain
/// the tempo curve integrity.
///
/// # Arguments
/// * `tempo_map` - The tempo envelope to modify
/// * `id` - The index of the tempo point to move (cannot be 0)
/// * `time_diff` - The time difference to move (in seconds)
/// * `check_edited_points` - Whether to validate that BPM values stay within legal range
///
/// # Returns
/// `true` if the move was successful, `false` if it would result in illegal values
pub fn move_tempo(
    tempo_map: &mut TempoEnvelope,
    id: usize,
    time_diff: f64,
    check_edited_points: bool,
) -> bool {
    // Can't move first point or if no movement
    if id == 0 || time_diff == 0.0 {
        return false;
    }

    // Get tempo points
    let Some(p2) = tempo_map.get_point(id).cloned() else {
        return false;
    };
    let Some(p1) = tempo_map.get_point(id - 1).cloned() else {
        return false;
    };
    let p3 = tempo_map.get_point(id + 1).cloned();

    let t1 = p1.time;
    let t2 = p2.time;
    let b1 = p1.bpm;
    let b2 = p2.bpm;
    let s1 = p1.shape;
    let s2 = p2.shape;

    let new_t2 = t2 + time_diff;

    // Calculate new BPM for current point
    let (new_b2, t3) = if let Some(ref p3) = p3 {
        let t3 = p3.time;
        let b3 = p3.bpm;

        let new_b2 = if s2 == SHAPE_SQUARE {
            b2 * (t3 - t2) / (t3 - new_t2)
        } else {
            (b2 + b3) * (t3 - t2) / (t3 - new_t2) - b3
        };

        (new_b2, t3)
    } else {
        // No next point - keep same BPM
        (b2, new_t2 + 1.0) // Fake t3 for validation
    };

    // Calculate new BPM for previous point
    let new_b1 = if s1 == SHAPE_SQUARE {
        b1 * (t2 - t1) / (new_t2 - t1)
    } else {
        (b1 + b2) * (t2 - t1) / (new_t2 - t1) - new_b2
    };

    // Check if values are legal
    if check_edited_points {
        if !(MIN_BPM..=MAX_BPM).contains(&new_b2) || !(MIN_BPM..=MAX_BPM).contains(&new_b1) {
            return false;
        }
        if (new_t2 - t1) < MIN_TEMPO_DIST || (t3 - new_t2) < MIN_TEMPO_DIST {
            return false;
        }
    }

    // Go through points backwards and calculate new values for linear points
    let mut prev_bpm: Vec<(usize, f64)> = Vec::new();
    let mut direction = 1.0;

    if id >= 2 {
        for i in (0..=(id - 2)).rev() {
            let Some(point) = tempo_map.get_point(i) else {
                break;
            };

            if point.shape == SHAPE_SQUARE {
                break;
            }

            let new_bpm = point.bpm - direction * (new_b1 - b1);

            if check_edited_points && !(MIN_BPM..=MAX_BPM).contains(&new_bpm) {
                return false;
            }

            prev_bpm.push((i, new_bpm));
            direction *= -1.0;
        }
    }

    // Apply all changes
    for (idx, bpm) in prev_bpm {
        tempo_map.set_point(idx, None, Some(bpm), None);
    }

    tempo_map.set_point(id - 1, None, Some(new_b1), None);
    tempo_map.set_point(id, Some(new_t2), Some(new_b2), None);

    true
}

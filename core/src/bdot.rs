use nalgebra::Vector3;

pub struct BdotAlgorithm {
    prev_field: Vector3<f32>,
}

impl BdotAlgorithm {
    pub fn new() -> Self {
        Self {
            prev_field: Vector3::zeros(),
        }
    }

    fn b_dot(&self, field: Vector3<f32>, dt: f32) -> Vector3<f32> {
        (field - self.prev_field) / dt
    }

    fn desired_dipole(&self, b_dot: Vector3<f32>) -> Vector3<f32> {
        if b_dot.norm() > f32::EPSILON {
            -b_dot.normalize()
        } else {
            Vector3::zeros()
        }
    }

    pub fn dipole_norm(&mut self, field: Vector3<f32>, dt: f32) -> Vector3<f32> {
        if dt <= f32::EPSILON || self.prev_field == Vector3::zeros() {
            self.prev_field = field;
            Vector3::zeros()
        } else {
            self.prev_field = field;
            let b_dot = self.b_dot(field, dt);
            self.desired_dipole(b_dot)
        }
    }

    pub fn reset(&mut self) {
        self.prev_field = Vector3::zeros();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_dipole_when_zero_field() {
        let mut bdot_alg = BdotAlgorithm::new();
        let result = bdot_alg.dipole_norm(Vector3::zeros(), 1.0);
        assert_eq!(result, Vector3::zeros());
    }

    #[test]
    fn zero_dipole_when_zero_dt() {
        let mut bdot_alg = BdotAlgorithm::new();
        let result = bdot_alg.dipole_norm(Vector3::zeros(), 0.0);
        assert_eq!(result, Vector3::zeros());
    }

    #[test]
    fn zero_dipole_when_static_field() {
        let mut bdot_alg = BdotAlgorithm::new();
        for _ in 0..3 {
            let result = bdot_alg.dipole_norm(Vector3::new(1.0, 0.0, 0.0), 1.0);
            assert_eq!(result, Vector3::zeros());
        }
    }
}

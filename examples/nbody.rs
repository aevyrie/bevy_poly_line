use std::time::Duration;

use bevy::{pbr::PointLightBundle, prelude::*};
use bevy_poly_line::{PolyLine, PolyLineBundle, PolyLineMaterial, PolyLinePlugin};

use lazy_static::*;
use rand::{prelude::*, Rng};
use ringbuffer::{ConstGenericRingBuffer, RingBufferExt, RingBufferWrite};

const NUM_BODIES: usize = 100;
const TRAIL_LENGTH: usize = 128;
const TRAIL_UPDATE_RATE_MILLIS: u64 = 25;

fn main() {
    let mut app = App::build();

    app.insert_resource(ClearColor(Color::BLACK))
        .insert_resource(Msaa { samples: 4 })
        .insert_resource(Timer::new(
            Duration::from_millis(TRAIL_UPDATE_RATE_MILLIS),
            true,
        ))
        .insert_resource(Simulation {
            scale: 1e5,
            ..Default::default()
        })
        .add_plugins(DefaultPlugins)
        .add_plugin(PolyLinePlugin)
        .add_startup_system(setup.system())
        .add_system(nbody_system.system())
        .add_system(rotator_system.system());

    app.run();
}

fn setup(mut commands: Commands, mut poly_line_materials: ResMut<Assets<PolyLineMaterial>>) {
    for _index in 0..NUM_BODIES {
        let mut rng = thread_rng();
        let position = Vec3::new(
            rng.gen_range(-100f32..100f32),
            rng.gen_range(-100f32..100f32),
            rng.gen_range(-100f32..100f32),
        );
        commands
            .spawn_bundle((
                Body {
                    mass: 1_000.0,
                    position,
                    ..Default::default()
                },
                ConstGenericRingBuffer::<Vec3, TRAIL_LENGTH>::new(),
            ))
            .insert_bundle(PolyLineBundle {
                poly_line: PolyLine {
                    vertices: Vec::with_capacity(TRAIL_LENGTH),
                },
                material: poly_line_materials.add(PolyLineMaterial {
                    width: 200.0,
                    color: Color::rgb_linear(
                        rng.gen_range(0.0..1.0),
                        rng.gen_range(0.0..1.0),
                        rng.gen_range(0.0..1.0),
                    ),
                    perspective: true,
                }),
                ..Default::default()
            });
    }

    // camera
    commands
        .spawn_bundle(PerspectiveCameraBundle {
            transform: Transform::from_xyz(0.0, 0.0, -500.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..PerspectiveCameraBundle::new_3d()
        })
        .insert(Rotates);
}

/// this component indicates what entities should rotate
struct Rotates;

fn rotator_system(time: Res<Time>, mut query: Query<&mut Transform, With<Rotates>>) {
    for mut transform in query.iter_mut() {
        *transform = Transform::from_rotation(Quat::from_rotation_y(
            (4.0 * std::f32::consts::PI / 20.0) * time.delta_seconds(),
        )) * *transform;
    }
}

#[derive(Clone, Debug, Default)]
struct Body {
    mass: f32,
    acceleration: Vec3,
    velocity: Vec3,
    position: Vec3,
}

#[derive(Debug)]
struct Simulation {
    pub accumulator: f32,
    pub seconds_since_startup: f64,
    pub is_paused: bool,
    pub scale: f32,
    pub timestep: f32,
}

impl Default for Simulation {
    fn default() -> Simulation {
        Simulation {
            seconds_since_startup: 0.0,
            accumulator: 0.0,
            is_paused: false,
            scale: 5e4,
            timestep: 1. / 30.,
        }
    }
}

impl Simulation {
    fn update(&mut self, time: &Time) {
        if !self.is_paused {
            self.accumulator += time.delta_seconds();
        }
    }

    fn step(&mut self) -> Option<f32> {
        if !self.is_paused && self.accumulator > self.timestep {
            self.accumulator -= self.timestep;
            return Some(self.timestep * self.scale);
        }
        None
    }
}

const G: f32 = 6.674_30E-11;
const EPSILON: f32 = 1.;

fn nbody_system(
    time: Res<Time>,
    mut timer: ResMut<Timer>,
    mut simulation: ResMut<Simulation>,
    mut query: Query<(
        Entity,
        &mut Body,
        &mut ConstGenericRingBuffer<Vec3, TRAIL_LENGTH>,
        &mut PolyLine,
    )>,
) {
    let mut bodies = query.iter_mut().collect::<Vec<_>>();
    // dbg!(&bodies);

    // Step simulation in fixed increments
    simulation.update(&*time);
    while let Some(dt) = simulation.step() {
        // Start substeps
        for substep in 0..3 {
            // Clear accelerations and update positions
            for (_, body, _, _) in bodies.iter_mut() {
                body.acceleration = Vec3::ZERO;
                let dx = (*CS)[substep] * body.velocity * dt;
                body.position += dx;
            }

            // Update accelerations
            for index1 in 0..bodies.len() {
                let (bodies1, bodies2) = bodies.split_at_mut(index1 + 1);
                let (_, body1, _, _) = &mut bodies1[index1];
                for (_, body2, _, _) in bodies2.iter_mut() {
                    let offset = body2.position - body1.position;
                    let distance_squared = offset.length_squared();
                    let normalized_offset = offset / distance_squared.sqrt();

                    let da = (G * body2.mass / (distance_squared + EPSILON)) * normalized_offset;
                    body1.acceleration += da;
                    body2.acceleration -= da;
                }
            }

            // Update velocities
            for (_, body, _, _) in bodies.iter_mut() {
                let dv = (*DS)[substep] * body.acceleration * dt;
                body.velocity += dv;
                if substep == 2 {
                    let dx = *C4 * body.velocity * dt;
                    body.position += dx;
                }
            }
        }
    }

    // Update Trails
    timer.tick(time.delta());
    if timer.just_finished() {
        bodies
            .iter_mut()
            .for_each(|(_entity, body, trail, poly_line)| {
                trail.push(body.position);
                poly_line.vertices = trail.to_vec();
            });
    }
}

lazy_static! {
    static ref W0: f32 = -2f32.cbrt() / (2f32 - 2f32.cbrt());
    static ref W1: f32 = 1f32 / (2f32 - 2f32.cbrt());
    static ref C1: f32 = *W1 / 2f32;
    static ref C2: f32 = (*W0 + *W1) / 2f32;
    static ref C3: f32 = *C2;
    static ref C4: f32 = *C1;
    static ref CS: [f32; 4] = [*C1, *C2, *C3, *C4];
    static ref D1: f32 = *W1;
    static ref D2: f32 = *W0;
    static ref D3: f32 = *D1;
    static ref DS: [f32; 3] = [*D1, *D2, *D3];
}

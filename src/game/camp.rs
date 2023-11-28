use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaChaRng;
use crate::AppState;
use crate::game::enemy;
use crate::Atlas;
use crate::map::{MAPSIZE, TILESIZE, CampNodes};
use crate::components::PowerUpType;
use crate::components::*;
use crate::Decorations;
use crate::Chests;
use crate::buffers::*;
use crate::game::map::setup_map;
use crate::map::MapSeed;
use crate::map::ChestCoords;

const CAMP_ENEMIES: u8 = 5;
const NUM_GRADES: u8 = 5;
const DEC_SIZE: Vec2 = Vec2 {x: 32., y: 32.};
const POWERUP_DROP_CHANCE: u32 = 50;
const CAMP_RESPAWN_TIME: f32 = 60.;

#[derive(Component)]
pub struct CampRespawnTimer(pub Timer);
const CHEST_SIZE: Vec2 = Vec2 {x: 32., y: 32.};

pub struct CampPlugin;

impl Plugin for CampPlugin{
    fn build(&self, app: &mut App){
        app.add_systems(OnEnter(AppState::Game), setup_camps
            .after(setup_map));
        app.add_systems(OnEnter(AppState::Game), setup_chests
            .after(setup_camps));
        app.add_systems(Update,(
            handle_camp_clear,
            respawn_camp_enemies,
        ));
    }
}

pub fn setup_camps(
    mut commands: Commands,
    entity_atlas:Res<Atlas>,
    camp_nodes: Res<CampNodes>,
    decoration_atlas: Res<Decorations>,
    map_seed: Res<MapSeed>,
    asset_server: Res<AssetServer>,
) {
    let mut rng = ChaChaRng::seed_from_u64(map_seed.0);
    // spawn a camp at a specified position

    //TODO: respawn enemies in a camp after a certain amount of time

    // Iterate through the MST of camps generated by perlin noise and spawn a camp at each node
    let mut campid: u8 = 0; 
    let mut id: u8 = 0;
    for camps in camp_nodes.0.iter(){
        // x-y position of the camp
        let camp_pos: Vec2 = get_spawn_vec(camps.x, camps.y);
        // determines camp/enemy type
        let camp_grade: u8 = rng.gen_range(1..=NUM_GRADES);
        //get the prefab data for the given grade
        let prefab_data = get_prefab_data(camp_grade);

        let special_enemy_index = rng.gen_range(0..CAMP_ENEMIES);

        commands.spawn((
            Camp(campid),
            SpatialBundle {
                transform: Transform::from_xyz(camp_pos.x, camp_pos.y, 0.),
                ..default()
            },
            Grade(camp_grade),
            CampEnemies{
                max_enemies: CAMP_ENEMIES,
                current_enemies: CAMP_ENEMIES,
            },
            CampStatus(true),
            CampRespawnTimer(Timer::from_seconds(CAMP_RESPAWN_TIME, TimerMode::Once)),
        ));

        let mut vec_counter = 0;

        // DECORATIONS NEED TO SPAWN BEFORE ENEMIES SO THAT THE VEC IS IN THE CORRECT ORDER
        //spawn decorations here
        for n in 0..3 {
            commands.spawn((
                SpriteSheetBundle {
                    texture_atlas: decoration_atlas.handle.clone(),
                    sprite: TextureAtlasSprite {index: decoration_atlas.coord_to_index(n, camp_grade as i32), ..default()},
                    transform: Transform{
                        translation: Vec3::new(
                            camp_pos.x + (prefab_data[vec_counter] * 16) as f32, 
                            camp_pos.y + (prefab_data[vec_counter + 1] * 16) as f32, 
                            2.
                        ),
                        scale: Vec3::new(2., 2., 0.),
                        ..default()
                    },
                    ..default()
                },
                Collider(DEC_SIZE),
            ));

            vec_counter+=2;
        }

        //spawn enemies for this camp
        for n in 0..CAMP_ENEMIES{
            let is_special = n == special_enemy_index;
            //generate a random powerup to drop from each enemy
            let powerups: [PowerUpType; 5] = [PowerUpType::MaxHPUp, PowerUpType::DamageDealtUp, PowerUpType::DamageReductionUp, PowerUpType::AttackSpeedUp, PowerUpType::MovementSpeedUp];
            //TODO: make this a random percentage based on the mapconfig resource
            let power_up_to_drop = powerups[camp_grade as usize - 1];
            let mut chance_drop_powerup = rng.gen_range(0..100) < POWERUP_DROP_CHANCE;

            if is_special{
                chance_drop_powerup = true;
            }

            enemy::spawn_enemy(
                &mut commands, 
                &asset_server,
                &entity_atlas, 
                id,
                campid, 
                Vec2::new(
                    camp_pos.x + (prefab_data[vec_counter] * 16) as f32, 
                    camp_pos.y + (prefab_data[vec_counter + 1] * 16) as f32), 
                camp_grade as i32, 
                power_up_to_drop,
                chance_drop_powerup,
                is_special,
            );
            vec_counter += 2;
        }
        campid += 1;
    }

}

pub fn setup_chests(
    mut commands: Commands,
    chest_coords: Res<ChestCoords>,
    map_seed: Res<MapSeed>,
    chest_atlas: Res<Chests>,
){

    // for chests in chest_coords, commands.spawn with chest component and health
    let mut rng = ChaChaRng::seed_from_u64(map_seed.0);
    let mut i = 0;
    
    for chest in chest_coords.0.iter(){
        let chest_pos: Vec2 = get_spawn_vec(chest.x, chest.y);

        let pb = PosBuffer(CircularBuffer::new_from(chest_pos));
        let new_chest = commands.spawn((
            ItemChest{
                id: i,
                // 5 random powerups
                contents: [rng.gen_range(0..5), rng.gen_range(0..5), rng.gen_range(0..5), rng.gen_range(0..5), rng.gen_range(0..5)]
            },
            pb,
            Health {
                current: 1,
                max: 1,
                dead: false,
            },
            Collider(CHEST_SIZE)
        )).id();
        commands.entity(new_chest).insert(SpriteSheetBundle{
            texture_atlas: chest_atlas.handle.clone(),
            sprite: TextureAtlasSprite { index: chest_atlas.coord_to_index(0, 1), ..Default::default()},
            transform: Transform { 
                translation: Vec3::new(chest_pos.x, chest_pos.y, 1.0),  
                scale: Vec3::new(2., 2., 0.),
                ..Default::default() 
            },
            ..Default::default()
        });

        i+=1;
    }
}

pub fn handle_camp_clear(
    mut camp_query: Query<(&CampEnemies, &mut CampStatus), With<Camp>>,
){
    for (enemies_in_camp, mut camp_status) in camp_query.iter_mut(){
        
        // only let this happen for camps that are currently active
        if camp_status.0 {
            //set the camp as cleared if all enemies are gone
            if enemies_in_camp.current_enemies == 0 {
                camp_status.0 = false;
            }
            
        }
    }
}

// convert given row and col into x and y coordinates. Returns a vec2 of these coordinates
fn get_spawn_vec(row: f32, col:f32) -> Vec2{
    let x_coord = TILESIZE as f32 * (row - (MAPSIZE as f32/2. + 0.5));
    let y_coord = TILESIZE as f32 * ((MAPSIZE as f32/2. - 0.5) - col);

    Vec2::new(x_coord, y_coord)
}

// given a grade, return a list of the attributes of that prefab
// LIST CONTENTS ARE:
/*
* dec 1 x offset = [0]
* dec 1 y offset = [1]
* dec 2 x offset = [2]
* dec 2 y offset = [3]
* dec 3 x offset = [4]
* dec 3 y offset = [5]
* en 1 x offset = [6]
* en 1 y offset = [7]
* en 2 x offset = [8]
* en 2 y offset = [9]
* en 3 x offset = [10]
* en 3 y offset = [11]
* en 4 x offset = [12]
* en 4 y offset = [13]
* en 5 x offset = [14]
* en 5 y offset = [15]
*/

fn get_prefab_data(grade: u8) -> Vec<i32>{

    let pd;
    match grade {
        1 => {
            pd = vec![0, 7, -5, -3, 3, -2, -3, 4, -2, -1, 3, 2, 6, 1, 0, -5];
        },
        2 => {
            pd = vec![-5, 4, 0, 1, 3, -4, -2, 7, -6, -3, -3, -6, 2, -3, 4, 3];
        },
        3 => {
            pd = vec![-1, 5, -3, -2, 3, -2, -4, 5, -2, 1, -6, 0, 4, 0, -4, -4];
        },
        4 => {
            pd = vec![-3, 4, 3, 2, 5, -1, 2, 6, -5, 1, -2, 1, -4, -3, 1, -6];
        },
        _ => {
            pd = vec![3, 3, 2, -2, -5, -4, 4, 6, -3, 4, 5, 0, -6, -2, -1, -4];
        },
    }
    pd
}

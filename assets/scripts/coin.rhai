// A spinning pickup. When the player touches it, it plays a sound, leaves a
// puff of sparks and disappears.

fn update(dt) {
    let turn = this.entity.rotation;
    turn.y += 120.0 * dt;
    this.entity.rotation = turn;
}

fn on_collision(other) {
    if other.name == "Player" {
        play_sound("sounds/pickup.wav", 0.8);
        spawn_scene("scenes/sparkle.ron", this.entity.position);
        this.entity.despawn();

        // despawn() takes effect at once, so this coin is no longer counted.
        let left = find_all("Coin").len();
        if left == 0 {
            print("All coins collected!");
            play_sound("sounds/win.wav", 0.8);
        } else {
            print(`${left} coins left`);
        }
    }
}

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Part of the tests in `mod.rs`.

use super::*;

#[cfg(unix)]
mod links {
    use super::*;
    use crate::skills::test_support::symlink;

    #[test]
    fn a_link_to_a_file_outside_the_skill_is_refused() {
        let scratch = Scratch::new("leak");
        let root = skill_dir(&scratch);
        symlink(
            &scratch.join("outside.txt"),
            &root.join("references/leak.md"),
        );
        assert_eq!(read(&root, "references/leak.md"), Err(Refusal::Escapes));
        // A relative link that climbs out.
        symlink(
            Path::new("../../../outside.txt"),
            &root.join("assets/up.txt"),
        );
        assert_eq!(read(&root, "assets/up.txt"), Err(Refusal::Escapes));
    }

    #[test]
    fn a_link_to_a_file_inside_the_skill_but_outside_the_readable_directory_is_refused() {
        let scratch = Scratch::new("sideways");
        let root = skill_dir(&scratch);
        symlink(
            &root.join("scripts/run.sh"),
            &root.join("references/script.txt"),
        );
        assert_eq!(read(&root, "references/script.txt"), Err(Refusal::Escapes));
        symlink(&root.join("SKILL.md"), &root.join("assets/skill.txt"));
        assert_eq!(read(&root, "assets/skill.txt"), Err(Refusal::Escapes));
        // And from references into assets: still not the directory asked for.
        symlink(
            &root.join("assets/template.txt"),
            &root.join("references/t.txt"),
        );
        assert_eq!(read(&root, "references/t.txt"), Err(Refusal::Escapes));
    }

    #[test]
    fn a_link_to_a_file_in_the_same_readable_directory_is_read() {
        let scratch = Scratch::new("samedir");
        let root = skill_dir(&scratch);
        symlink(
            &root.join("references/guide.md"),
            &root.join("references/alias.md"),
        );
        assert_eq!(
            read(&root, "references/alias.md").as_deref(),
            Ok("the guide")
        );
    }

    #[test]
    fn a_readable_directory_that_is_a_link_is_not_followed() {
        let scratch = Scratch::new("dirlink");
        let root = skill_dir(&scratch);
        scratch.write("shared/doc.md", "shared");
        std::fs::remove_dir_all(root.join("assets")).expect("removes");
        symlink(&scratch.join("shared"), &root.join("assets"));
        assert_eq!(read(&root, "assets/doc.md"), Err(Refusal::Escapes));
        // Even to a directory inside the skill.
        std::fs::remove_file(root.join("assets")).expect("removes");
        symlink(&root.join("references"), &root.join("assets"));
        assert_eq!(read(&root, "assets/guide.md"), Err(Refusal::Escapes));
    }

    #[test]
    fn a_loop_of_links_is_unreadable_not_followed_forever() {
        let scratch = Scratch::new("loop");
        let root = skill_dir(&scratch);
        symlink(Path::new("b.md"), &root.join("references/a.md"));
        symlink(Path::new("a.md"), &root.join("references/b.md"));
        assert!(matches!(
            read(&root, "references/a.md"),
            Err(Refusal::Unreadable(_))
        ));
        symlink(Path::new("self"), &root.join("references/self"));
        assert!(matches!(
            read(&root, "references/self"),
            Err(Refusal::Unreadable(_))
        ));
    }

    #[test]
    fn a_dangling_link_is_not_found() {
        let scratch = Scratch::new("dangling");
        let root = skill_dir(&scratch);
        symlink(
            Path::new("nowhere.md"),
            &root.join("references/dangling.md"),
        );
        assert_eq!(
            read(&root, "references/dangling.md"),
            Err(Refusal::NotFound)
        );
    }

    #[test]
    fn a_skill_md_that_is_a_link_out_of_the_directory_is_refused() {
        let scratch = Scratch::new("skillmdlink");
        scratch.write("secret.md", "---\nname: x\ndescription: d\n---\nleaked");
        std::fs::create_dir_all(scratch.join("skills/demo")).expect("creates");
        symlink(
            &scratch.join("secret.md"),
            &scratch.join("skills/demo/SKILL.md"),
        );
        let root = scratch
            .join("skills/demo")
            .canonicalize()
            .expect("resolves");
        assert_eq!(read_skill_file(&root, 1024), Err(Refusal::Escapes));
    }

    #[test]
    fn a_skill_md_that_is_a_link_inside_the_directory_is_read() {
        let scratch = Scratch::new("skillmdinside");
        scratch.write(
            "skills/demo/real.md",
            "---\nname: demo\ndescription: d\n---\nok",
        );
        symlink(Path::new("real.md"), &scratch.join("skills/demo/SKILL.md"));
        let root = scratch
            .join("skills/demo")
            .canonicalize()
            .expect("resolves");
        assert!(read_skill_file(&root, 1024).is_ok());
    }

    /// The window between resolving a path and opening it: what `verify`
    /// sees when the file opened is not the file at the path.
    #[test]
    fn a_file_other_than_the_one_at_the_path_is_caught_after_the_open() {
        let scratch = Scratch::new("swap");
        let root = skill_dir(&scratch);
        let boundary = root.join("references");
        let a = boundary.join("guide.md");
        let b = boundary.join("deep/er/note.md");
        let opened_a = File::open(&a).expect("opens");
        assert_eq!(verify(&opened_a, &a, &boundary), Ok(()));
        assert_eq!(
            verify(&opened_a, &b, &boundary),
            Err(Refusal::ChangedWhileOpening),
            "the handle is a's and the path is b's"
        );
        // The same, in the way an attacker would: the path is replaced after
        // the open by a link out of the directory.
        std::fs::remove_file(&a).expect("removes");
        symlink(&scratch.join("outside.txt"), &a);
        assert_eq!(
            verify(&opened_a, &a, &boundary),
            Err(Refusal::ChangedWhileOpening)
        );
        // Or by another file under the same name.
        std::fs::remove_file(&a).expect("removes");
        scratch.write("skills/demo/references/guide.md", "replaced");
        assert_eq!(
            verify(&opened_a, &a, &boundary),
            Err(Refusal::ChangedWhileOpening)
        );
    }

    #[test]
    fn a_directory_opened_in_place_of_a_file_is_caught() {
        let scratch = Scratch::new("diropen");
        let root = skill_dir(&scratch);
        let boundary = root.join("references");
        let directory = boundary.join("deep");
        let handle = File::open(&directory).expect("a directory opens on this platform");
        assert_eq!(
            verify(&handle, &directory, &boundary),
            Err(Refusal::ChangedWhileOpening)
        );
    }
}

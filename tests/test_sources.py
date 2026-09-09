import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile

SPEC = importlib.util.spec_from_file_location('winbuild', Path(__file__).resolve().parents[1] / 'winbuild.py')
winbuild = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(winbuild)


class SourceArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.project = self.root / "project with ' spaces"
        self.project.mkdir()
        self.git('init', '-q')
        self.git('config', 'user.email', 'test@example.invalid')
        self.git('config', 'user.name', 'Build Machine Tests')
        (self.project / '.gitignore').write_text('secret/\nnode_modules/\n')
        (self.project / 'source.txt').write_text('committed\n')
        self.git('add', '.gitignore', 'source.txt')
        self.git('commit', '-qm', 'Initial test input')

    def git(self, *args):
        subprocess.run(['git', '-C', str(self.project), *args], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def archive(self, name='source.zip'):
        path = self.root / name
        result = winbuild.make_source_archive(self.project, path)
        return result, path

    def test_current_edits_and_untracked_files_are_included_but_ignored_data_is_not(self):
        (self.project / 'source.txt').write_text('current edit\n')
        (self.project / 'new.txt').write_text('new input\n')
        (self.project / 'secret').mkdir()
        (self.project / 'secret' / 'private.txt').write_text('must not transfer')
        result, path = self.archive()
        with zipfile.ZipFile(path) as archive:
            self.assertEqual(archive.read('source.txt'), b'current edit\n')
            self.assertEqual(archive.read('new.txt'), b'new input\n')
            self.assertNotIn('secret/private.txt', archive.namelist())
        self.assertTrue(result['dirty'])

    def test_same_contents_produce_same_hash_and_edits_invalidate_it(self):
        first, _ = self.archive('first.zip')
        (self.project / 'source.txt').touch()
        second, _ = self.archive('second.zip')
        self.assertEqual(first['sourceHash'], second['sourceHash'])
        (self.project / 'source.txt').write_text('changed')
        third, _ = self.archive('third.zip')
        self.assertNotEqual(first['sourceHash'], third['sourceHash'])

    def test_tracked_deletion_is_not_restored_from_git(self):
        (self.project / 'source.txt').unlink()
        _, path = self.archive()
        with zipfile.ZipFile(path) as archive:
            self.assertNotIn('source.txt', archive.namelist())

    def test_symlink_cannot_export_files_outside_project(self):
        outside = self.root / 'private.txt'
        outside.write_text('private')
        (self.project / 'outside.txt').symlink_to(outside)
        with self.assertRaisesRegex(ValueError, 'outside the project'):
            self.archive()

    def test_projects_with_same_name_have_separate_destinations(self):
        self.assertNotEqual(winbuild.project_key(Path('/first/app')), winbuild.project_key(Path('/second/app')))

    def test_repository_subdirectory_is_not_a_project_root(self):
        nested = self.project / 'src'
        nested.mkdir()
        with self.assertRaisesRegex(ValueError, '루트'):
            winbuild.git_root(nested)


if __name__ == '__main__':
    unittest.main()

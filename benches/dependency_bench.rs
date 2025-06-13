use criterion::{criterion_group, criterion_main, Criterion};
use gget::dependency::DependencyResolver;
use std::hint::black_box;

fn bench_extract_dependencies(c: &mut Criterion) {
    let mut resolver = DependencyResolver::new().unwrap();

    // 테스트용 Go 소스 코드
    let source_code = r#"
    package main

    import (
        "gno.land/p/demo/avl"
        "gno.land/p/demo/ufmt"
        "gno.land/r/demo/users"
        _ "gno.land/p/demo/blank"
    )

    func main() {
        // some code
    }
    "#;

    c.bench_function("extract_dependencies", |b| {
        b.iter(|| black_box(resolver.extract_dependencies(black_box(source_code))).unwrap())
    });
}

fn bench_extract_dependencies_large_file(c: &mut Criterion) {
    let mut resolver = DependencyResolver::new().unwrap();

    // 더 큰 테스트 파일 생성
    let mut large_source = String::from("package main\n\nimport (\n");
    for i in 0..100 {
        large_source.push_str(&format!(r#"    "gno.land/p/demo/import{}""#, i));
        if i < 99 {
            large_source.push_str(",\n");
        }
    }
    large_source.push_str("\n)\n\nfunc main() {\n    // some code\n}\n");

    c.bench_function("extract_dependencies_large_file", |b| {
        b.iter(|| black_box(resolver.extract_dependencies(black_box(&large_source))).unwrap())
    });
}

criterion_group!(
    benches,
    bench_extract_dependencies,
    bench_extract_dependencies_large_file
);
criterion_main!(benches);
